use std::future::Future;

use crate::image_hash::stream::{stream, Inbox};
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

    pub fn run(self) -> (Inbox, impl Future<Output = ()>) {
        let mut boss_stream = self.handler.subscribe_boss_updates();
        let Updater {
            hasher,
            handler,
            log,
            ..
        } = self;
        let (inbox, hashes) = stream(hasher, self.concurrency);
        let mut hashes = Box::pin(hashes);

        let hash_inbox = inbox.clone();
        let hash_requester = async move {
            while let Some(entry) = boss_stream.next().await {
                let boss = entry.boss();
                if boss.image_hash.is_some() {
                    continue;
                }

                for lang in &[Language::English, Language::Japanese] {
                    if let (Some(name), Some(image_url)) =
                        (boss.name.get(*lang), boss.image.get(*lang))
                    {
                        if let Ok(uri) = image_url.parse() {
                            hash_inbox.request_hash(name.clone(), uri);
                        }
                    }
                }
            }
        };

        let hash_updater = async move {
            while let Some(item) = hashes.next().await {
                match item.image_hash {
                    Ok(image_hash) => handler.update_image_hash(&item.boss_name, image_hash),
                    Err(e) => slog::warn!(
                        log, "Failed to get image hash";
                        "error" => %e, "bossName" => %item.boss_name
                    ),
                }
            }
        };

        let worker = futures::future::join(hash_requester, hash_updater).map(|_| ());
        (inbox, worker)
    }
}
