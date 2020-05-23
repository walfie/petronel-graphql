use std::sync::Arc;

use crate::error::Result;
use crate::image_hash::{ImageHash, ImageHasher};
use crate::model::{Boss, BossName, Language};

use dashmap::DashMap;
use futures::stream::{Stream, StreamExt};
use futures::FutureExt;
use http::Uri;
use tokio::sync::mpsc;

#[derive(Debug)]
pub struct BossImageHash {
    pub boss_name: BossName,
    pub image_hash: Result<ImageHash>,
}

#[derive(Debug, Clone)]
pub struct Inbox(mpsc::UnboundedSender<(BossName, Uri)>);
impl Inbox {
    pub fn request_hash(&self, boss_name: BossName, uri: Uri) {
        let _ = self.0.send((boss_name, uri));
    }

    pub fn request_hash_for_boss(&self, boss: &Boss) {
        for lang in Language::VALUES {
            if let (Some(name), Some(image)) = (boss.name.get(*lang), boss.image.get(*lang)) {
                if let Ok(url) = image.parse() {
                    let _ = self.0.send((name.clone(), url));
                }
            }
        }
    }
}

pub fn stream<H>(image_hasher: H, concurrency: usize) -> (Inbox, impl Stream<Item = BossImageHash>)
where
    H: ImageHasher + Send + Sync + 'static,
{
    let (tx_in, mut rx_in) = mpsc::unbounded_channel::<(BossName, Uri)>();
    let (tx_out, rx_out) = mpsc::unbounded_channel();

    let image_hasher = Arc::new(image_hasher);

    let worker = async move {
        let requested = Arc::new(DashMap::new());
        let image_hasher = image_hasher.clone();

        while let Some((boss_name, uri)) = rx_in.recv().await {
            let requested = requested.clone();

            if requested.contains_key(&boss_name) {
                continue;
            }

            requested.insert(boss_name.clone(), ());

            let image_hasher = image_hasher.clone();
            let future = async move {
                let image_hash = image_hasher.hash(uri).await;
                if image_hash.is_err() {
                    requested.clone().remove(&boss_name);
                }

                BossImageHash {
                    boss_name,
                    image_hash,
                }
            };

            if let Err(_) = tx_out.send(future) {
                break; // Listener dropped
            }
        }
    };

    let output = futures::stream::select(
        rx_out.buffer_unordered(concurrency),
        worker
            .into_stream()
            .filter_map(|()| futures::future::ready(None)),
    );

    (Inbox(tx_in), output)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::error::{Error, Result};
    use async_trait::async_trait;
    use http::StatusCode;

    struct MockImageHasher;

    #[async_trait]
    impl ImageHasher for MockImageHasher {
        async fn hash(&self, uri: Uri) -> Result<ImageHash> {
            match uri.to_string().as_ref() {
                "http://example.com/image1.png" => Ok(ImageHash(1)),
                "http://example.com/image2.png" => Ok(ImageHash(2)),
                "http://example.com/image3.png" => Err(Error::Http(StatusCode::NOT_FOUND)),
                _ => panic!(),
            }
        }
    }

    #[tokio::test]
    async fn test_stream() -> anyhow::Result<()> {
        let (tx, rx) = stream(MockImageHasher, 5);
        let mut rx = Box::pin(rx);

        // Request each boss 3 times
        for _ in 0..3usize {
            tx.request_hash(
                "Boss1".into(),
                "http://example.com/image1.png".parse().unwrap(),
            );
        }

        for _ in 0..3usize {
            tx.request_hash(
                "Boss2".into(),
                "http://example.com/image2.png".parse().unwrap(),
            );
        }

        for _ in 0..3usize {
            tx.request_hash(
                "Boss3".into(),
                "http://example.com/image3.png".parse().unwrap(),
            );
        }

        // Should receive each successful hash result only once
        let next = rx.next().await.unwrap();
        assert_eq!(&next.boss_name, "Boss1");
        assert_eq!(next.image_hash.unwrap(), ImageHash(1));

        let next = rx.next().await.unwrap();
        assert_eq!(&next.boss_name, "Boss2");
        assert_eq!(next.image_hash.unwrap(), ImageHash(2));

        let next = rx.next().await.unwrap();
        assert_eq!(&next.boss_name, "Boss3");
        assert!(matches!(
            next.image_hash,
            Err(Error::Http(StatusCode::NOT_FOUND))
        ));

        // Request hashes for all the images again
        tx.request_hash(
            "Boss1".into(),
            "http://example.com/image1.png".parse().unwrap(),
        );

        tx.request_hash(
            "Boss2".into(),
            "http://example.com/image2.png".parse().unwrap(),
        );

        tx.request_hash(
            "Boss3".into(),
            "http://example.com/image3.png".parse().unwrap(),
        );

        // The hasher should ignore previously successful requests,
        // and retry only ones that fail
        assert!(matches!(
            rx.next().await.unwrap().image_hash,
            Err(Error::Http(StatusCode::NOT_FOUND))
        ));

        // On drop, the stream should end
        drop(tx);
        assert!(rx.next().await.is_none());

        Ok(())
    }
}
