use std::sync::Arc;

use crate::error::Result;
use crate::image_hash::{ImageHash, ImageHasher};
use crate::model::{Boss, BossName, Language};

use dashmap::DashMap;
use futures::future::Either;
use futures::stream::{Stream, StreamExt};
use futures::FutureExt;
use http::Uri;
use tokio::sync::mpsc;

#[derive(Debug)]
pub struct BossImageHash {
    pub boss_name: BossName,
    pub image_hash: Result<ImageHash>,
}

/// Inbox for requesting image hashes
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

    // Represents whether we're still waiting for an image hash
    enum State {
        Pending,
        Success(ImageHash),
        Failure,
    }

    let worker = async move {
        // On success, store the completed value in `requested`,
        // so that future requests can avoid having to recompute the hash
        let requested = Arc::new(DashMap::<BossName, State>::new());
        let image_hasher = image_hasher.clone();

        while let Some((boss_name, uri)) = rx_in.recv().await {
            let requested = requested.clone();

            if let Some(guard) = requested.get(&boss_name) {
                match guard.value() {
                    State::Pending => {
                        // There's already a pending request for this boss, don't re-submit
                        continue;
                    }
                    State::Failure => {
                        // The last attempt failed, so we can retry
                    }
                    State::Success(image_hash) => {
                        // Reuse the previous successful result, don't re-submit
                        let hash = BossImageHash {
                            boss_name: boss_name.clone(),
                            image_hash: Ok(*image_hash),
                        };

                        let future = futures::future::ready(hash);
                        if let Err(_) = tx_out.send(Either::Left(future)) {
                            break; // Listener dropped
                        }

                        continue;
                    }
                }
            }

            requested.insert(boss_name.clone(), State::Pending);

            let image_hasher = image_hasher.clone();
            let future = async move {
                let image_hash = image_hasher.hash(uri).await;

                let state = match image_hash {
                    Ok(hash) => State::Success(hash),
                    Err(_) => State::Failure,
                };

                requested.insert(boss_name.clone(), state);

                BossImageHash {
                    boss_name,
                    image_hash,
                }
            };

            if let Err(_) = tx_out.send(Either::Right(future)) {
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
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering::SeqCst;

    struct MockImageHasher {
        image1_requested: AtomicUsize,
        image2_requested: AtomicUsize,
        image3_requested: AtomicUsize,
    }

    impl MockImageHasher {
        fn new() -> Self {
            Self {
                image1_requested: AtomicUsize::new(0),
                image2_requested: AtomicUsize::new(0),
                image3_requested: AtomicUsize::new(0),
            }
        }
    }

    const IMAGE1: &'static str = "http://example.com/image1.png";
    const IMAGE2: &'static str = "http://example.com/image2.png";
    const IMAGE3: &'static str = "http://example.com/image3.png";

    #[async_trait]
    impl ImageHasher for MockImageHasher {
        async fn hash(&self, uri: Uri) -> Result<ImageHash> {
            // Asserting that once an image hash is successful, the hash is
            // never computed again (the last successful value is reused)
            match uri.to_string().as_ref() {
                IMAGE1 => {
                    self.image1_requested.fetch_add(1, SeqCst);
                    match self.image1_requested.load(SeqCst) {
                        1 => Ok(ImageHash(1)),
                        _ => unreachable!(),
                    }
                }
                IMAGE2 => {
                    self.image2_requested.fetch_add(1, SeqCst);
                    match self.image2_requested.load(SeqCst) {
                        1 => Ok(ImageHash(2)),
                        _ => unreachable!(),
                    }
                }
                IMAGE3 => {
                    self.image3_requested.fetch_add(1, SeqCst);
                    match self.image3_requested.load(SeqCst) {
                        1 => Err(Error::Http(StatusCode::INTERNAL_SERVER_ERROR)),
                        2 => Err(Error::Http(StatusCode::SERVICE_UNAVAILABLE)),
                        3 => Ok(ImageHash(3)),
                        _ => unreachable!(),
                    }
                }
                _ => unreachable!(),
            }
        }
    }

    #[tokio::test]
    async fn test_stream() -> anyhow::Result<()> {
        let (tx, rx) = stream(MockImageHasher::new(), 5);
        let mut rx = Box::pin(rx);

        // Request each boss 3 times
        for _ in 0..3usize {
            tx.request_hash("Boss1".into(), IMAGE1.parse().unwrap());
            tx.request_hash("Boss2".into(), IMAGE2.parse().unwrap());
            tx.request_hash("Boss3".into(), IMAGE3.parse().unwrap());
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
            Err(Error::Http(StatusCode::INTERNAL_SERVER_ERROR))
        ));

        // Request hashes for all the images again
        tx.request_hash("Boss1".into(), IMAGE1.parse().unwrap());
        tx.request_hash("Boss2".into(), IMAGE2.parse().unwrap());
        tx.request_hash("Boss3".into(), IMAGE3.parse().unwrap());

        // The hasher should reuse previously successful attempts
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
            Err(Error::Http(StatusCode::SERVICE_UNAVAILABLE))
        ));

        // Retry boss3 again, and it should succeed
        tx.request_hash("Boss3".into(), IMAGE3.parse().unwrap());

        let next = rx.next().await.unwrap();
        assert_eq!(&next.boss_name, "Boss3");
        assert_eq!(next.image_hash.unwrap(), ImageHash(3));

        // Retry once more, and it should reuse the successful value
        tx.request_hash("Boss3".into(), IMAGE3.parse().unwrap());

        let next = rx.next().await.unwrap();
        assert_eq!(&next.boss_name, "Boss3");
        assert_eq!(next.image_hash.unwrap(), ImageHash(3));

        // On drop, the stream should end
        drop(tx);
        assert!(rx.next().await.is_none());

        Ok(())
    }
}
