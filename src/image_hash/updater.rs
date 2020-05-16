use std::future::Future;

use crate::image_hash::stream::stream;
use crate::image_hash::ImageHasher;
use crate::model::Language;
use crate::raid_handler::RaidHandler;

use futures::stream::StreamExt;
use futures::FutureExt;

pub struct Updater<H> {
    log: slog::Logger,
    hasher: H,
    handler: RaidHandler,
    concurrency: usize,
}

impl<H> Updater<H>
where
    H: ImageHasher + Send + Sync + 'static,
{
    pub fn new(log: slog::Logger, hasher: H, handler: RaidHandler, concurrency: usize) -> Self {
        Self {
            log,
            hasher,
            handler,
            concurrency,
        }
    }

    pub fn run(self) -> impl Future<Output = ()> {
        let mut boss_stream = self.handler.subscribe_boss_updates();
        let Updater {
            hasher,
            handler,
            log,
            ..
        } = self;
        let (inbox, hashes) = stream(hasher, self.concurrency);
        let mut hashes = Box::pin(hashes);

        let hash_requester = async move {
            while let Some(boss) = boss_stream.next().await {
                if boss.image_hash.is_some() {
                    continue;
                }

                for lang in &[Language::English, Language::Japanese] {
                    if let (Some(name), Some(image_url)) =
                        (boss.name.get(*lang), boss.image.get(*lang))
                    {
                        if let Ok(uri) = image_url.parse() {
                            inbox.request_hash(name.clone(), uri);
                        }
                    }
                }
            }
        };

        let hash_updater = async move {
            while let Some(result) = hashes.next().await {
                match result {
                    Ok(item) => handler.update_image_hash(item.boss_name, item.image_hash),
                    Err(e) => slog::warn!(log, "Failed to get image hash"; "error" => %e),
                }
            }
        };

        futures::future::join(hash_requester, hash_updater).map(|_| ())
    }
}
