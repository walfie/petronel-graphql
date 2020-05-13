use std::ops::Deref;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use crate::model::{Boss, BossName, CachedString, Language, Raid};

use circular_queue::CircularQueue;
use dashmap::DashMap;
use futures_util::stream::Stream;
use parking_lot::RwLock;
use tokio::sync::broadcast;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct BossKey(BossName);

#[derive(Clone, Debug)]
pub struct RaidHandler(Arc<RaidHandlerInner>);

pin_project_lite::pin_project! {
    pub struct Subscription {
        #[pin]
        rx: broadcast::Receiver<Arc<Raid>>,
        boss_name: BossName,
        handler: Arc<RaidHandlerInner>,
    }
}

impl Stream for Subscription {
    type Item = Arc<Raid>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        loop {
            match futures_util::ready!(this.rx.as_mut().poll_next(cx)) {
                Some(Ok(item)) => return Poll::Ready(Some(item)),
                Some(Err(broadcast::RecvError::Lagged(_))) => continue,
                Some(Err(broadcast::RecvError::Closed)) => (),
                None => (),
            }

            // Attempt to resubscribe if the inner subscription ends
            // (e.g., if the stream is stopped due to being merged with another boss)
            std::mem::replace(
                &mut *this.rx,
                this.handler.subscribe(this.boss_name.clone()),
            );
        }
    }
}

impl RaidHandler {
    pub fn new(capacity: usize) -> Self {
        Self(Arc::new(RaidHandlerInner::new(capacity)))
    }

    pub fn subscribe(&self, boss_name: BossName) -> Subscription {
        let inner = self.0.clone();

        Subscription {
            rx: inner.subscribe(boss_name.clone()),
            boss_name,
            handler: inner.clone(),
        }
    }
}

impl Deref for RaidHandler {
    type Target = RaidHandlerInner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug)]
pub struct RaidHandlerInner {
    bosses: DashMap<BossKey, Arc<Boss>>,
    raid_history: DashMap<BossKey, RwLock<CircularQueue<Arc<Raid>>>>,
    raid_broadcast: DashMap<BossKey, broadcast::Sender<Arc<Raid>>>,
    boss_broadcast: broadcast::Sender<Arc<Boss>>,
    translations: DashMap<CachedString, CachedString>,
    capacity: usize,
}

impl RaidHandlerInner {
    // TODO: Initialize with existing bosses
    fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);

        Self {
            bosses: DashMap::new(),
            raid_history: DashMap::new(),
            raid_broadcast: DashMap::new(),
            translations: DashMap::new(),
            boss_broadcast: tx,
            capacity,
        }
    }

    fn subscribe(&self, boss_name: BossName) -> broadcast::Receiver<Arc<Raid>> {
        let key = self.boss_key(boss_name);

        if let Some(guard) = self.raid_broadcast.get(&key) {
            guard.value().subscribe()
        } else {
            let (tx, rx) = broadcast::channel(self.capacity);
            self.raid_broadcast.insert(key, tx);
            rx
        }
    }

    fn boss_key(&self, boss_name: BossName) -> BossKey {
        match self.translations.get(&boss_name) {
            None => BossKey(boss_name),
            Some(elem) => BossKey(elem.value().clone()),
        }
    }

    pub fn get_history(&self, boss_name: BossName) -> Vec<Arc<Raid>> {
        let key = self.boss_key(boss_name);
        if let Some(guard) = self.raid_history.get(&key) {
            guard.value().read().iter().cloned().collect::<Vec<_>>()
        } else {
            Vec::new()
        }
    }

    pub fn boss(&self, name: BossName) -> Option<Arc<Boss>> {
        self.bosses
            .get(&self.boss_key(name))
            .map(|guard| guard.value().clone())
    }

    pub fn bosses(&self) -> Vec<Arc<Boss>> {
        self.bosses
            .iter()
            .map(|guard| guard.value().clone())
            .collect::<Vec<_>>()
    }

    pub fn merge(&self, src: BossName, dst: BossName) {
        let src_key = BossKey(src.clone());
        let dst_key = BossKey(dst.clone());

        if let (Some(src_boss), Some(dst_boss)) =
            (self.bosses.get(&src_key), self.bosses.get(&dst_key))
        {
            let mut boss = Boss::clone(&dst_boss);
            boss.name = boss.name.merge(&src_boss.name);
            boss.image = boss.image.merge(&src_boss.image);
            let boss = Arc::new(boss);
            self.bosses.insert(dst_key.clone(), boss.clone());
            self.bosses.remove(&src_key);

            // Combine the raid history of the two bosses
            if let Some(src_history) = self.raid_history.remove_take(&src_key) {
                if let Some(dst_history) = self.raid_history.get(&dst_key) {
                    let mut combined_history = src_history
                        .value()
                        .read()
                        .asc_iter()
                        .cloned()
                        .collect::<Vec<_>>();

                    let dst_history = dst_history.value();

                    combined_history.extend(dst_history.read().asc_iter().cloned());
                    combined_history.sort_by_key(|raid| raid.created_at);

                    combined_history
                        .drain(..)
                        .for_each(|raid| dst_history.write().push(raid));
                }
            }

            self.translations.insert(src, dst);
            self.raid_broadcast.remove(&src_key);

            let _ = self.boss_broadcast.send(boss);
        }
    }

    pub fn push(&self, raid: Raid) {
        let key = if raid.language == Language::Japanese {
            BossKey(raid.boss_name.clone())
        } else {
            self.boss_key(raid.boss_name.clone())
        };

        let raid = Arc::new(raid);

        // Broadcast the raid to all listeners of this boss
        if let Some(guard) = self.raid_broadcast.get(&key) {
            let _ = guard.value().send(raid.clone());
        }

        if let Some(guard) = self.bosses.get(&key) {
            // TODO: Update last_seen_at
            let boss = guard.value();

            // If the incoming raid has an image URL but the existing boss doesn't, update the image
            if boss.image.get(raid.language).is_none() && raid.image_url.is_some() {
                let mut updated_boss = Boss::clone(&boss);
                updated_boss
                    .image
                    .set(raid.language, raid.image_url.clone());

                let updated_boss_arc = Arc::new(updated_boss);

                let _ = self.boss_broadcast.send(updated_boss_arc.clone());
                let _ = self.bosses.insert(key.clone(), updated_boss_arc);
            }
        } else {
            let boss = Arc::new(Boss::from(raid.as_ref()));
            let _ = self.boss_broadcast.send(boss.clone());
            self.bosses.insert(key.clone(), boss);
        }

        // Update raid history
        if let Some(guard) = self.raid_history.get(&key) {
            guard.value().write().push(raid.clone());
        } else {
            let mut queue = CircularQueue::with_capacity(self.capacity);
            queue.push(raid);
            let queue = RwLock::new(queue);

            self.raid_history.insert(key.clone(), queue);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::model::LangString;
    use chrono::offset::TimeZone;
    use chrono::Utc;
    use futures_util::stream::StreamExt;

    #[tokio::test]
    async fn get_history() {
        let capacity = 2;

        let handler = RaidHandler::new(capacity);
        let boss_name_ja = BossName::from("Lv60 オオゾラッコ");
        let boss_name_en = BossName::from("Lvl 60 Ozorotter");

        let mut subscriber_ja = handler.subscribe(boss_name_ja.clone());
        let mut subscriber_en = handler.subscribe(boss_name_en.clone());

        let raid1 = Raid {
            id: "1".into(),
            tweet_id: 1,
            user_name: "walfieee".into(),
            user_image: None,
            boss_name: boss_name_ja.clone(),
            created_at: Utc.ymd(2020, 5, 20).and_hms(1, 2, 3),
            text: Some("Help".into()),
            language: Language::Japanese,
            image_url: None,
        };

        assert!(handler.get_history(boss_name_ja.clone()).is_empty());
        assert!(handler.get_history(boss_name_en.clone()).is_empty());
        assert!(handler.bosses().is_empty());

        handler.push(raid1.clone());
        assert_eq!(
            handler.get_history(boss_name_ja.clone()),
            vec![Arc::new(raid1.clone())]
        );
        assert_eq!(handler.bosses(), vec![Arc::new(Boss::from(&raid1))]);
        assert_eq!(subscriber_ja.next().await.unwrap(), Arc::new(raid1.clone()));

        let mut raid2 = raid1.clone();
        raid2.id = "2".into();
        raid2.tweet_id = 2;
        raid2.created_at = raid1.created_at + chrono::Duration::minutes(1);
        raid2.text = None;

        // Items should be returned latest first
        handler.push(raid2.clone());
        assert_eq!(
            handler.get_history(boss_name_ja.clone()),
            vec![Arc::new(raid2.clone()), Arc::new(raid1.clone())]
        );
        assert_eq!(subscriber_ja.next().await.unwrap(), Arc::new(raid2.clone()));

        // When capacity is full, old entries should be overwritten
        let mut raid3 = raid2.clone();
        raid3.id = "3".into();
        raid3.tweet_id = 3;
        raid3.created_at = raid2.created_at + chrono::Duration::minutes(1);
        handler.push(raid3.clone());
        assert_eq!(
            handler.get_history(boss_name_ja.clone()),
            vec![Arc::new(raid3.clone()), Arc::new(raid2.clone())]
        );
        assert_eq!(subscriber_ja.next().await.unwrap(), Arc::new(raid3.clone()));

        // Push a raid from a boss with a different name
        let mut raid4 = raid3.clone();
        raid4.id = "4".into();
        raid4.tweet_id = 4;
        raid4.boss_name = boss_name_en.clone();
        raid4.language = Language::English;
        raid4.created_at = raid3.created_at + chrono::Duration::minutes(1);
        handler.push(raid4.clone());
        assert_eq!(
            handler.get_history(boss_name_en.clone()),
            vec![Arc::new(raid4.clone())]
        );
        assert_eq!(subscriber_en.next().await.unwrap(), Arc::new(raid4.clone()));

        // Merge the two bosses. The history should be merged, as well as the boss entries and broadcast.
        handler.merge(boss_name_en.clone(), boss_name_ja.clone());
        assert_eq!(
            handler.get_history(boss_name_en.clone()),
            vec![Arc::new(raid4.clone()), Arc::new(raid3.clone())]
        );
        assert_eq!(
            handler.get_history(boss_name_ja.clone()),
            vec![Arc::new(raid4.clone()), Arc::new(raid3.clone())]
        );

        let expected_boss = Arc::new(Boss {
            name: LangString {
                en: Some(boss_name_en.clone()),
                ja: Some(boss_name_ja.clone()),
            },
            image: LangString {
                en: raid4.image_url.as_ref().cloned(),
                ja: raid1.image_url.as_ref().cloned(),
            },
            ..Boss::from(&raid1)
        });
        assert_eq!(
            handler.boss(boss_name_en.clone()),
            Some(expected_boss.clone())
        );
        assert_eq!(handler.boss(boss_name_ja.clone()), Some(expected_boss));

        // The next raid should get sent to both `en` and `ja` subscribers
        let mut raid5 = raid4.clone();
        raid5.id = "5".into();
        raid5.tweet_id = 5;
        raid5.boss_name = boss_name_ja.clone();
        raid5.created_at = raid4.created_at + chrono::Duration::minutes(1);
        {
            let raid5 = raid5.clone();
            let handler = handler.clone();
            tokio::spawn(async move {
                // Arbitrarily chosen delay to make the test work.
                // Not sure of a good workaround here.
                tokio::time::delay_for(std::time::Duration::from_millis(500)).await;
                handler.push(raid5);
            });
        }
        assert_eq!(subscriber_en.next().await.unwrap(), Arc::new(raid5.clone()));
        assert_eq!(subscriber_ja.next().await.unwrap(), Arc::new(raid5.clone()));

        let mut raid6 = raid5.clone();
        raid6.id = "6".into();
        raid6.tweet_id = 6;
        raid6.boss_name = boss_name_en.clone();
        raid6.created_at = raid5.created_at + chrono::Duration::minutes(1);
        handler.push(raid6.clone());
        assert_eq!(subscriber_en.next().await.unwrap(), Arc::new(raid6.clone()));
        assert_eq!(subscriber_ja.next().await.unwrap(), Arc::new(raid6.clone()));
    }
}
