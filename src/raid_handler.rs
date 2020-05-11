use std::sync::{Arc, Mutex, RwLock};

use crate::model::{Boss, BossName, CachedString, Language, Raid};

use dashmap::DashMap;
use tokio::sync::broadcast;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct BossKey(BossName);

pub struct RaidHandler {
    bosses: DashMap<BossKey, Arc<Boss>>,
    raid_history: DashMap<BossKey, RaidHistory>,
    raid_broadcast: DashMap<BossKey, broadcast::Sender<Arc<Raid>>>,
    boss_broadcast: broadcast::Sender<Arc<Boss>>,
    translations: DashMap<CachedString, CachedString>,
    capacity: usize,
}

struct RaidHistory {
    tx: Mutex<ringbuf::Producer<Arc<Raid>>>,
    rx: RwLock<ringbuf::Consumer<Arc<Raid>>>,
}

impl RaidHandler {
    // TODO: Initialize with existing bosses
    pub fn new(capacity: usize) -> Self {
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

    fn boss_key(&self, boss_name: BossName) -> BossKey {
        match self.translations.get(&boss_name) {
            None => BossKey(boss_name),
            Some(elem) => BossKey(elem.value().clone()),
        }
    }

    fn boss_key_lang(&self, lang: Language, boss_name: BossName) -> BossKey {
        if lang == Language::Japanese {
            return BossKey(boss_name);
        }

        self.boss_key(boss_name)
    }

    pub fn get_history(&self, boss_name: BossName) -> Vec<Arc<Raid>> {
        let key = self.boss_key(boss_name);
        if let Some(guard) = self.raid_history.get(&key) {
            let mut out = Vec::with_capacity(self.capacity);
            guard.value().rx.read().unwrap().access(|slice1, slice2| {
                slice2.iter().rev().for_each(|item| out.push(item.clone()));
                slice1.iter().rev().for_each(|item| out.push(item.clone()));
            });
            out
        } else {
            Vec::new()
        }
    }

    pub fn push(&self, raid: Raid) {
        let key = self.boss_key_lang(raid.language, raid.boss_name.clone());
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
            let value = guard.value();
            let mut rx = value.rx.write().unwrap();
            if rx.is_full() {
                let _ = rx.pop();
            }
            std::mem::drop(rx);
            let _ = value.tx.lock().unwrap().push(raid.clone());
        } else {
            let (mut tx, rx) = ringbuf::RingBuffer::new(self.capacity).split();
            let _ = tx.push(raid);
            let tx = Mutex::new(tx);
            let rx = RwLock::new(rx);

            self.raid_history
                .insert(key.clone(), RaidHistory { tx, rx });
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use chrono::offset::TimeZone;
    use chrono::Utc;

    #[test]
    fn get_history() {
        let capacity = 2;

        let handler = RaidHandler::new(capacity);

        let boss_name: BossName = "Lvl 60 Ozorotter".into();
        let raid1 = Raid {
            id: "1".into(),
            tweet_id: 1,
            user_name: "walfieee".into(),
            user_image: None,
            boss_name: boss_name.clone(),
            created_at: Utc.ymd(2020, 5, 20).and_hms(1, 2, 3),
            text: Some("Help".into()),
            language: Language::English,
            image_url: None,
        };

        assert!(handler.get_history(boss_name.clone()).is_empty());

        handler.push(raid1.clone());
        assert_eq!(
            handler.get_history(boss_name.clone()),
            vec![Arc::new(raid1.clone())]
        );

        let mut raid2 = raid1.clone();
        raid2.id = "2".into();
        raid2.tweet_id = 2;
        raid2.user_name = "walfie_game".into();
        raid2.text = None;

        // Items should be returned in reverse order
        handler.push(raid2.clone());
        assert_eq!(
            handler.get_history(boss_name.clone()),
            vec![Arc::new(raid2.clone()), Arc::new(raid1.clone())]
        );

        let mut raid3 = raid2.clone();
        raid3.id = "3".into();
        raid3.tweet_id = 3;
        handler.push(raid3.clone());
        assert_eq!(
            handler.get_history(boss_name.clone()),
            vec![Arc::new(raid3.clone()), Arc::new(raid2.clone())]
        );
    }
}
