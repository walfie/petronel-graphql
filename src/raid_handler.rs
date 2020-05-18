use std::borrow::Borrow;
use std::ops::Deref;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use crate::model::{Boss, BossName, CachedString, ImageHash, Language, Raid};

use circular_queue::CircularQueue;
use dashmap::DashMap;
use futures::stream::Stream;
use parking_lot::RwLock;
use tokio::stream::StreamExt;
use tokio::sync::broadcast;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct BossKey(pub(crate) BossName);

impl Borrow<str> for BossKey {
    fn borrow(&self) -> &str {
        self.0.as_ref()
    }
}

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
            match futures::ready!(this.rx.as_mut().poll_next(cx)) {
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
    // TODO:
    // * Initialize with existing bosses, persistence
    // * Manual translation override for Lvl 120 Medusa
    // * Cleanup of bosses with no subscribers
    // * Split capacity into separate values for history and stream backlog
    // * Prometheus metrics
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

    pub fn subscribe_boss_updates(&self) -> impl Stream<Item = Arc<Boss>> {
        self.boss_broadcast.subscribe().filter_map(Result::ok)
    }

    fn boss_key(&self, boss_name: BossName) -> BossKey {
        match self.translations.get(&boss_name) {
            None => BossKey(boss_name),
            Some(elem) => BossKey(elem.value().clone()),
        }
    }

    pub fn history(&self, boss_name: BossName) -> Vec<Arc<Raid>> {
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

    pub fn update_image_hash(&self, name: BossName, image_hash: ImageHash) {
        let key = self.boss_key(name.clone());
        if let Some(guard) = self.bosses.get(&key) {
            let boss = guard.value();
            if boss.image_hash == Some(image_hash) {
                return; // Do nothing, it's already set
            }

            let mut boss = Boss::clone(&boss);
            boss.image_hash = Some(image_hash);
            let boss = Arc::new(boss);
            self.bosses.insert(key, boss.clone());

            let is_japanese = boss.name.ja.is_some();

            // Iterate through boss list to find a boss with the same image hash and level
            for item in self.bosses.iter() {
                let other_boss_name = &item.key().0;
                let other_boss = item.value();
                if other_boss.image_hash == Some(image_hash)
                    && other_boss.level == boss.level
                    && other_boss_name != &name
                {
                    if is_japanese {
                        self.merge(other_boss_name.clone(), name);
                    } else {
                        self.merge(name, other_boss_name.clone());
                    }
                    break;
                }
            }
        }
    }

    // `dst` should be the Japanese boss name
    // TODO: Make this less error-prone
    fn merge(&self, src: BossName, dst: BossName) {
        let src_key = BossKey(src.clone());
        let dst_key = BossKey(dst.clone());

        if let (Some(src_boss), Some(dst_boss)) =
            (self.bosses.get(&src_key), self.bosses.get(&dst_key))
        {
            let mut boss = Boss::clone(&dst_boss);
            boss.name = boss.name.merge(&src_boss.name);
            boss.image = boss.image.merge(&src_boss.image);
            boss.last_seen_at = (&dst_boss.last_seen_at).max(&src_boss.last_seen_at).clone();
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
            let boss = guard.value();
            boss.last_seen_at.replace(&raid.created_at);

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
    use futures::stream::StreamExt;

    const BOSS_NAME_JA: &'static str = "Lv60 オオゾラッコ";
    const BOSS_NAME_EN: &'static str = "Lvl 60 Ozorotter";

    #[tokio::test]
    async fn get_history() {
        use Language::{English, Japanese};

        let capacity = 2;

        let handler = RaidHandler::new(capacity);

        let mut subscriber_ja = handler.subscribe(BOSS_NAME_JA.into());
        let mut subscriber_en = handler.subscribe(BOSS_NAME_EN.into());
        let mut boss_subscriber = handler.subscribe_boss_updates();

        fn next(raid: &Raid, language: Language) -> Raid {
            let mut raid = raid.clone();
            raid.tweet_id += 1;
            raid.id = raid.tweet_id.to_string().into();
            raid.created_at = raid.created_at + chrono::Duration::seconds(1);
            raid.language = language;
            raid.boss_name = match language {
                Japanese => BOSS_NAME_JA.into(),
                English => BOSS_NAME_EN.into(),
            };
            raid
        }

        let raid1 = Raid {
            id: "1".into(),
            tweet_id: 1,
            user_name: "walfieee".into(),
            user_image: None,
            boss_name: BOSS_NAME_JA.into(),
            created_at: Utc.ymd(2020, 5, 20).and_hms(1, 2, 3),
            text: Some("Help".into()),
            language: Language::Japanese,
            image_url: None,
        };

        assert!(handler.history(BOSS_NAME_JA.into()).is_empty());
        assert!(handler.history(BOSS_NAME_EN.into()).is_empty());
        assert!(handler.bosses().is_empty());

        handler.push(raid1.clone());
        assert_eq!(
            handler.history(BOSS_NAME_JA.into()),
            vec![Arc::new(raid1.clone())]
        );
        assert_eq!(handler.bosses(), vec![Arc::new(Boss::from(&raid1))]);
        assert_eq!(subscriber_ja.next().await.unwrap(), Arc::new(raid1.clone()));
        assert_eq!(
            boss_subscriber.next().await.unwrap(),
            Arc::new(Boss::from(&raid1))
        );

        let raid2 = next(&raid1, Japanese);

        // Items should be returned latest first
        handler.push(raid2.clone());
        assert_eq!(
            handler.history(BOSS_NAME_JA.into()),
            vec![Arc::new(raid2.clone()), Arc::new(raid1.clone())]
        );
        assert_eq!(subscriber_ja.next().await.unwrap(), Arc::new(raid2.clone()));

        // When capacity is full, old entries should be overwritten
        let raid3 = next(&raid2, Japanese);
        handler.push(raid3.clone());
        assert_eq!(
            handler.history(BOSS_NAME_JA.into()),
            vec![Arc::new(raid3.clone()), Arc::new(raid2.clone())]
        );
        assert_eq!(subscriber_ja.next().await.unwrap(), Arc::new(raid3.clone()));

        // Push a raid from a boss with a different name
        let raid4 = next(&raid3, English);
        handler.push(raid4.clone());
        assert_eq!(
            handler.history(BOSS_NAME_EN.into()),
            vec![Arc::new(raid4.clone())]
        );
        assert_eq!(subscriber_en.next().await.unwrap(), Arc::new(raid4.clone()));
        assert_eq!(
            boss_subscriber.next().await.unwrap(),
            Arc::new(Boss::from(&raid4))
        );

        // Merge the two bosses. The history should be merged, as well as the boss entries and broadcast.
        handler.update_image_hash(BOSS_NAME_EN.into(), ImageHash(123));
        handler.update_image_hash(BOSS_NAME_JA.into(), ImageHash(123));

        let expected_boss = Arc::new(Boss {
            name: LangString {
                en: Some(BOSS_NAME_EN.into()),
                ja: Some(BOSS_NAME_JA.into()),
            },
            image: LangString {
                en: raid4.image_url.as_ref().cloned(),
                ja: raid1.image_url.as_ref().cloned(),
            },
            image_hash: Some(ImageHash(123)),
            ..Boss::from(&raid4)
        });
        assert_eq!(
            handler.history(BOSS_NAME_EN.into()),
            vec![Arc::new(raid4.clone()), Arc::new(raid3.clone())]
        );
        assert_eq!(
            handler.history(BOSS_NAME_JA.into()),
            vec![Arc::new(raid4.clone()), Arc::new(raid3.clone())]
        );

        assert_eq!(handler.boss(BOSS_NAME_EN.into()).unwrap(), expected_boss);
        assert_eq!(handler.boss(BOSS_NAME_JA.into()).unwrap(), expected_boss);
        assert_eq!(boss_subscriber.next().await.unwrap(), expected_boss);

        // The next raid should get sent to `en` and `ja` subscribers, including new ones
        let mut subscriber_en2 = handler.subscribe(BOSS_NAME_EN.into());
        let mut subscriber_ja2 = handler.subscribe(BOSS_NAME_JA.into());
        let raid5 = next(&raid4, Japanese);
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
        let expected = Some(Arc::new(raid5.clone()));
        assert_eq!(subscriber_en.next().await, expected);
        assert_eq!(subscriber_en2.next().await, expected);
        assert_eq!(subscriber_ja.next().await, expected);
        assert_eq!(subscriber_ja2.next().await, expected);

        // English boss name should also go to both subscribers
        let raid6 = next(&raid5, English);
        handler.push(raid6.clone());
        let expected = Some(Arc::new(raid6.clone()));
        assert_eq!(subscriber_en.next().await, expected);
        assert_eq!(subscriber_en2.next().await, expected);
        assert_eq!(subscriber_ja.next().await, expected);
        assert_eq!(subscriber_ja2.next().await, expected);
    }
}
