use std::ops::Deref;
use std::pin::Pin;
use std::sync::{Arc, Weak};
use std::task::{Context, Poll};

use crate::metrics::{
    LangMetric, Metric, MetricFactory, Metrics, PrometheusMetric, PrometheusMetricFactory,
};
use crate::model::{Boss, BossName, CachedString, ImageHash, NodeId, Raid};

use arc_swap::ArcSwap;
use circular_queue::CircularQueue;
use dashmap::{DashMap, ElementGuard};
use futures::stream::Stream;
use parking_lot::RwLock;
use tokio::stream::StreamExt;
use tokio::sync::broadcast;

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
            std::mem::replace(&mut *this.rx, this.handler.subscribe(&this.boss_name));
        }
    }
}

impl RaidHandler {
    pub fn new(
        metric_factory: PrometheusMetricFactory,
        bosses: Vec<Boss>,
        history_size: usize,
        broadcast_capacity: usize,
    ) -> Self {
        Self(Arc::new(RaidHandlerInner::new(
            metric_factory,
            bosses,
            history_size,
            broadcast_capacity,
        )))
    }

    pub fn subscribe(&self, boss_name: BossName) -> Subscription {
        let inner = self.0.clone();

        Subscription {
            rx: inner.subscribe(&boss_name),
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
pub struct BossEntry {
    node_id: CachedString,
    boss: Boss,
    history: RwLock<CircularQueue<Arc<Raid>>>,
    broadcast: broadcast::Sender<Arc<Raid>>,
    tweet_count: LangMetric<PrometheusMetric>,
    subscriber_count: PrometheusMetric,
}

impl Clone for BossEntry {
    fn clone(&self) -> Self {
        let boss = self.boss.clone();

        let history = RwLock::new(self.history.read().clone());

        BossEntry {
            node_id: self.node_id.clone(),
            boss,
            history,
            broadcast: self.broadcast.clone(),
            tweet_count: self.tweet_count.clone(),
            subscriber_count: self.subscriber_count.clone(),
        }
    }
}

impl BossEntry {
    #[inline]
    pub fn node_id(&self) -> &CachedString {
        &self.node_id
    }

    #[inline]
    pub fn boss(&self) -> &Boss {
        &self.boss
    }

    // Ideally this would return a value that doesn't leak implementation details,
    // but I can't figure out a great way to do it
    pub fn history(&self) -> &RwLock<CircularQueue<Arc<Raid>>> {
        &self.history
    }
}

pub struct Bosses(arc_swap::Guard<'static, Arc<Vec<Arc<BossEntry>>>>);
impl Deref for Bosses {
    type Target = Vec<Arc<BossEntry>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug)]
pub struct RaidHandlerInner {
    metric_factory: PrometheusMetricFactory,
    bosses: BossMap,
    boss_broadcast: broadcast::Sender<Weak<BossEntry>>,
    history_size: usize,
    broadcast_capacity: usize,
}

#[derive(Debug)]
struct BossMap {
    map: DashMap<CachedString, Arc<BossEntry>>,
    // Bosses sorted by level, then name
    vec: ArcSwap<Vec<Arc<BossEntry>>>,
    // Bosses that don't exist yet, but are subscribed to
    waiting: DashMap<CachedString, broadcast::Sender<Arc<Raid>>>,
    history_size: usize,
    broadcast_capacity: usize,
}

impl BossMap {
    fn new(
        metric_factory: &PrometheusMetricFactory,
        mut bosses: Vec<Boss>,
        history_size: usize,
        broadcast_capacity: usize,
    ) -> Self {
        bosses.sort_by_key(|boss| boss.name.canonical().cloned());
        bosses.dedup_by(|a, b| a.name == b.name);

        let mut init = Vec::new();

        for boss in bosses {
            let (tx, _) = broadcast::channel(broadcast_capacity);
            let entry = Arc::new(BossEntry {
                node_id: NodeId::from_boss_name(&boss.name).to_string().into(),
                history: RwLock::new(CircularQueue::with_capacity(history_size)),
                broadcast: tx,
                tweet_count: metric_factory.boss_tweet_counter(&boss.name),
                subscriber_count: metric_factory.boss_subscriber_gauge(&boss.name),
                boss,
            });

            entry
                .boss
                .name
                .for_each(|name| init.push((name.clone(), entry.clone())));
        }

        let this = Self {
            map: DashMap::from_iter(init),
            vec: ArcSwap::from_pointee(Vec::new()),
            waiting: DashMap::new(),
            history_size,
            broadcast_capacity,
        };

        this.update_vec();
        this
    }

    fn get(&self, name: &CachedString) -> Option<ElementGuard<CachedString, Arc<BossEntry>>> {
        self.map.get(name)
    }

    fn update_vec(&self) {
        let mut vec = self
            .map
            .iter()
            .map(|guard| guard.value().clone())
            .collect::<Vec<_>>();

        vec.sort_by_key(|entry| (entry.boss.level, entry.boss.name.canonical().cloned()));
        vec.dedup_by(|a, b| Arc::ptr_eq(a, b));

        self.vec.store(Arc::new(vec));
    }

    fn retain(&self, predicate: impl FnMut(&CachedString, &Arc<BossEntry>) -> bool) {
        let len = self.map.len();
        self.map.retain(predicate);
        if self.map.len() != len {
            self.update_vec();
        }

        self.waiting.retain(|_k, v| v.receiver_count() > 0);
    }

    fn find(
        &self,
        predicate: impl FnMut(&ElementGuard<CachedString, Arc<BossEntry>>) -> bool,
    ) -> Option<ElementGuard<CachedString, Arc<BossEntry>>> {
        self.map.iter().find(predicate)
    }

    fn as_vec(&self) -> &ArcSwap<Vec<Arc<BossEntry>>> {
        &self.vec
    }

    fn insert(&self, entry: &Arc<BossEntry>) {
        entry.boss.name.for_each(|name| {
            self.map.insert(name.clone(), entry.clone());
            self.waiting.remove(&name);
        });

        self.update_vec();
    }

    fn subscribe(&self, key: &CachedString) -> broadcast::Receiver<Arc<Raid>> {
        if let Some(guard) = self.map.get(key) {
            guard.value().broadcast.subscribe()
        } else if let Some(guard) = self.waiting.get(key) {
            guard.value().subscribe()
        } else {
            let (tx, rx) = broadcast::channel(self.broadcast_capacity);
            self.waiting.insert(key.into(), tx);
            rx
        }
    }

    fn new_entry_from_raid(
        &self,
        metric_factory: &PrometheusMetricFactory,
        raid: Raid,
    ) -> Arc<BossEntry> {
        let boss = Boss::from(&raid);
        let broadcast = if let Some(tx) = self.waiting.remove_take(&raid.boss_name) {
            tx.value().clone()
        } else {
            let (tx, _) = broadcast::channel(self.broadcast_capacity);
            tx
        };

        let entry = BossEntry {
            node_id: NodeId::from_boss_name(&boss.name).to_string().into(),
            history: RwLock::new(CircularQueue::with_capacity(self.history_size)),
            broadcast,
            tweet_count: metric_factory.boss_tweet_counter(&boss.name),
            subscriber_count: metric_factory.boss_subscriber_gauge(&boss.name),
            boss,
        };

        let raid = Arc::new(raid);
        let _ = entry.broadcast.send(raid.clone());
        entry.history.write().push(raid.clone());

        let entry = Arc::new(entry);
        self.insert(&entry);
        entry
    }
}

impl RaidHandlerInner {
    fn new(
        metric_factory: PrometheusMetricFactory,
        bosses: Vec<Boss>,
        history_size: usize,
        broadcast_capacity: usize,
    ) -> Self {
        let (tx, _) = broadcast::channel(broadcast_capacity);

        Self {
            bosses: BossMap::new(&metric_factory, bosses, history_size, broadcast_capacity),
            boss_broadcast: tx,
            history_size,
            broadcast_capacity,
            metric_factory,
        }
    }

    fn subscribe(&self, boss_name: &CachedString) -> broadcast::Receiver<Arc<Raid>> {
        self.bosses.subscribe(boss_name)
    }

    pub fn retain(&self, mut predicate: impl FnMut(&Arc<BossEntry>) -> bool) {
        self.bosses.retain(|_k, v| predicate(v));
    }

    pub fn subscribe_boss_updates(&self) -> impl Stream<Item = Arc<BossEntry>> {
        self.boss_broadcast
            .subscribe()
            .filter_map(|entry| entry.ok().and_then(|w| w.upgrade()))
    }

    pub fn boss(&self, name: &CachedString) -> Option<Arc<BossEntry>> {
        self.bosses.get(name).map(|guard| guard.value().clone())
    }

    pub fn bosses(&self) -> Bosses {
        Bosses(self.bosses.as_vec().load())
    }

    pub fn metrics(&self) -> <PrometheusMetricFactory as MetricFactory>::Output {
        let bosses = self.bosses();

        let mut metrics = Metrics {
            boss_tweet_counters: Vec::with_capacity(bosses.len()),
            boss_subscriber_gauges: Vec::with_capacity(bosses.len()),
        };

        for boss in bosses.iter() {
            metrics.boss_tweet_counters.push(&boss.tweet_count);
            boss.subscriber_count.set(boss.broadcast.receiver_count());
            metrics.boss_subscriber_gauges.push(&boss.subscriber_count);
        }

        self.metric_factory.write(&metrics)
    }

    pub fn update_image_hash(&self, boss_name: &BossName, image_hash: ImageHash) {
        let guard = match self.bosses.get(boss_name) {
            Some(g) => g,
            None => return,
        };

        let boss_entry = guard.value();

        let this_boss = &boss_entry.boss;

        if this_boss.image_hash.is_some() {
            return; // Do nothing, it's already set
        }

        let is_japanese = this_boss.name.ja.is_some();

        let matching_entry_opt = self.bosses.find(|item| {
            let value = item.value();
            let other_boss = &value.boss;

            other_boss.image_hash == Some(image_hash)
                && other_boss.level == this_boss.level
                && other_boss.name != this_boss.name
        });

        if let Some(matching_entry) = matching_entry_opt {
            let other_entry = matching_entry.value();

            // Merge the two entries, keeping values from the Japanese one
            let (entry_to_keep, entry_to_discard) = if is_japanese {
                (boss_entry, other_entry)
            } else {
                (other_entry, boss_entry)
            };

            let mut merged_boss = Boss::clone(&entry_to_keep.boss);
            merged_boss.name = entry_to_keep.boss.name.merge(&entry_to_discard.boss.name);
            merged_boss.image = entry_to_keep.boss.image.merge(&entry_to_discard.boss.image);
            merged_boss.image_hash = Some(image_hash);
            merged_boss.last_seen_at = std::cmp::max(
                entry_to_keep.boss.last_seen_at.clone(),
                entry_to_discard.boss.last_seen_at.clone(),
            );

            let mut new_history = CircularQueue::with_capacity(self.history_size);
            let mut combined_history = entry_to_discard
                .history
                .read()
                .asc_iter()
                .cloned()
                .collect::<Vec<_>>();
            combined_history.extend(entry_to_keep.history.read().asc_iter().cloned());
            combined_history.sort_by_key(|raid| *raid.created_at.as_datetime());
            combined_history
                .drain(..)
                .for_each(|raid| new_history.push(raid));

            let new_entry = Arc::new(BossEntry {
                node_id: NodeId::from_boss_name(&merged_boss.name).to_string().into(),
                history: RwLock::new(new_history),
                broadcast: entry_to_keep.broadcast.clone(),
                tweet_count: self.metric_factory.boss_tweet_counter(&merged_boss.name),
                subscriber_count: self.metric_factory.boss_subscriber_gauge(&merged_boss.name),
                boss: merged_boss,
            });

            self.bosses.insert(&new_entry);

            let _ = self.boss_broadcast.send(Arc::downgrade(&new_entry));
        } else {
            let mut new_entry = BossEntry::clone(boss_entry);
            new_entry.boss.image_hash = Some(image_hash);
            self.bosses.insert(&Arc::new(new_entry));
        }
    }

    pub fn push(&self, raid: Raid) {
        if let Some(guard) = self.bosses.get(&raid.boss_name) {
            let entry = guard.value();

            entry
                .boss
                .last_seen_at
                .replace(raid.created_at.as_datetime());

            let raid = Arc::new(raid);

            // Broadcast the raid to all listeners of this boss and update history
            let _ = entry.broadcast.send(raid.clone());
            entry.history.write().push(raid.clone());

            // Update metrics
            entry.tweet_count.get(raid.language).inc();

            // If the incoming raid has an image URL but the existing boss doesn't, update the image
            if entry.boss.image.get(raid.language).is_none() && raid.image_url.is_some() {
                // Update the existing entry with a new hash
                let mut new_entry = BossEntry::clone(entry);
                new_entry
                    .boss
                    .image
                    .set(raid.language, raid.image_url.clone());
                let new_entry = Arc::new(new_entry);

                self.bosses.insert(&new_entry);
                let _ = self.boss_broadcast.send(Arc::downgrade(&new_entry));
            }
        } else {
            let entry = self.bosses.new_entry_from_raid(&self.metric_factory, raid);
            let _ = self.boss_broadcast.send(Arc::downgrade(&entry));
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::model::{LangString, Language};
    use chrono::offset::TimeZone;
    use chrono::Utc;
    use futures::stream::StreamExt;
    use once_cell::sync::Lazy;

    const BOSS_NAME_JA: Lazy<BossName> = Lazy::new(|| "Lv60 オオゾラッコ".into());
    const BOSS_NAME_EN: Lazy<BossName> = Lazy::new(|| "Lvl 60 Ozorotter".into());

    fn get_history(handler: &RaidHandler, boss_name: &BossName) -> Vec<Arc<Raid>> {
        match handler.boss(boss_name) {
            None => Vec::new(),
            Some(entry) => entry.history().read().iter().cloned().collect(),
        }
    }

    fn get_bosses(handler: &RaidHandler) -> Vec<Boss> {
        handler
            .bosses()
            .iter()
            .map(|entry| entry.boss.clone())
            .collect()
    }

    #[tokio::test]
    async fn scenario() {
        use Language::{English, Japanese};

        let history_size = 2;
        let broadcast_capacity = 10;
        let metric_factory = PrometheusMetricFactory::new("petronel".to_owned());

        let handler =
            RaidHandler::new(metric_factory, Vec::new(), history_size, broadcast_capacity);

        let mut subscriber_ja = handler.subscribe(BOSS_NAME_JA.clone());
        let mut subscriber_en = handler.subscribe(BOSS_NAME_EN.clone());
        let mut boss_subscriber = handler.subscribe_boss_updates();

        fn next(raid: &Raid, language: Language) -> Raid {
            let mut raid = raid.clone();
            raid.tweet_id += 1;
            raid.id = raid.tweet_id.to_string().into();
            raid.created_at =
                (raid.created_at.as_datetime().clone() + chrono::Duration::seconds(1)).into();
            raid.language = language;
            raid.boss_name = match language {
                Japanese => BOSS_NAME_JA.clone(),
                English => BOSS_NAME_EN.clone(),
            };
            raid
        }

        let raid1 = Raid {
            id: "1".into(),
            tweet_id: 1,
            user_name: "walfieee".into(),
            user_image: None,
            boss_name: BOSS_NAME_JA.clone(),
            created_at: Utc.ymd(2020, 5, 20).and_hms(1, 2, 3).into(),
            text: Some("Help".into()),
            language: Language::Japanese,
            image_url: None,
        };

        assert!(handler.boss(&BOSS_NAME_JA).is_none());
        assert!(handler.boss(&BOSS_NAME_EN).is_none());
        assert!(handler.bosses().is_empty());

        handler.push(raid1.clone());
        assert_eq!(subscriber_ja.next().await.unwrap(), Arc::new(raid1.clone()));
        assert_eq!(
            get_history(&handler, &BOSS_NAME_JA),
            vec![Arc::new(raid1.clone())]
        );
        assert_eq!(get_bosses(&handler), vec![Boss::from(&raid1)]);
        assert_eq!(
            boss_subscriber.next().await.unwrap().boss,
            Boss::from(&raid1)
        );

        let raid2 = next(&raid1, Japanese);

        // Items should be returned latest first
        handler.push(raid2.clone());
        assert_eq!(
            get_history(&handler, &BOSS_NAME_JA),
            vec![Arc::new(raid2.clone()), Arc::new(raid1.clone())]
        );
        assert_eq!(subscriber_ja.next().await.unwrap(), Arc::new(raid2.clone()));

        // When capacity is full, old entries should be overwritten
        let raid3 = next(&raid2, Japanese);
        handler.push(raid3.clone());
        assert_eq!(
            get_history(&handler, &BOSS_NAME_JA),
            vec![Arc::new(raid3.clone()), Arc::new(raid2.clone())]
        );
        assert_eq!(subscriber_ja.next().await.unwrap(), Arc::new(raid3.clone()));

        // Push a raid from a boss with a different name
        let raid4 = next(&raid3, English);
        handler.push(raid4.clone());
        assert_eq!(subscriber_en.next().await.unwrap(), Arc::new(raid4.clone()));
        assert_eq!(
            get_history(&handler, &BOSS_NAME_EN),
            vec![Arc::new(raid4.clone())]
        );
        assert_eq!(
            boss_subscriber.next().await.unwrap().boss,
            Boss::from(&raid4)
        );

        // Merge the two bosses. The history should be merged, as well as the boss entries and broadcast.
        handler.update_image_hash(&BOSS_NAME_EN, ImageHash(123));
        handler.update_image_hash(&BOSS_NAME_JA, ImageHash(123));

        let expected_boss = Boss {
            name: LangString {
                en: Some(BOSS_NAME_EN.clone()),
                ja: Some(BOSS_NAME_JA.clone()),
            },
            image: LangString {
                en: raid4.image_url.as_ref().cloned(),
                ja: raid1.image_url.as_ref().cloned(),
            },
            image_hash: Some(ImageHash(123)),
            ..Boss::from(&raid4)
        };
        assert_eq!(
            get_history(&handler, &BOSS_NAME_EN),
            vec![Arc::new(raid4.clone()), Arc::new(raid3.clone())]
        );
        assert_eq!(
            get_history(&handler, &BOSS_NAME_JA),
            vec![Arc::new(raid4.clone()), Arc::new(raid3.clone())]
        );

        assert_eq!(handler.boss(&BOSS_NAME_EN).unwrap().boss, expected_boss);
        assert_eq!(handler.boss(&BOSS_NAME_JA).unwrap().boss, expected_boss);
        assert_eq!(boss_subscriber.next().await.unwrap().boss, expected_boss);

        // The next raid should get sent to `en` and `ja` subscribers, including new ones
        let mut subscriber_en2 = handler.subscribe(BOSS_NAME_EN.clone());
        let mut subscriber_ja2 = handler.subscribe(BOSS_NAME_JA.clone());
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
