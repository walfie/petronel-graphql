use chrono::offset::{TimeZone, Utc};
use once_cell::sync::Lazy;
use regex::Regex;
use std::cmp::Ordering;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering::Relaxed;

pub type CachedString = string_cache::DefaultAtom;
pub type BossName = CachedString;
pub type DateTime = chrono::DateTime<Utc>;
pub type Level = i16;
pub type TweetId = u64;
pub type RaidId = String;

#[derive(Clone, Debug, PartialEq)]
pub struct Boss {
    pub name: LangString,
    pub image: LangString,
    pub level: Option<Level>,
    pub last_seen_at: AtomicDateTime,
    // TODO: Image hash
}

#[derive(Clone, Debug, PartialEq)]
pub struct Raid {
    pub id: RaidId,
    pub tweet_id: TweetId,
    pub user_name: String,
    pub user_image: Option<String>,
    pub boss_name: BossName,
    pub created_at: DateTime,
    pub text: Option<String>,
    pub language: Language,
    pub image_url: Option<CachedString>,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Language {
    Japanese,
    English,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LangString {
    pub en: Option<CachedString>,
    pub ja: Option<CachedString>,
}

impl LangString {
    pub fn empty() -> Self {
        Self { en: None, ja: None }
    }

    pub fn get(&self, lang: Language) -> Option<&CachedString> {
        match lang {
            Language::English => self.en.as_ref(),
            Language::Japanese => self.ja.as_ref(),
        }
    }

    pub fn set(&mut self, lang: Language, value: Option<CachedString>) {
        match lang {
            Language::English => self.en = value,
            Language::Japanese => self.ja = value,
        }
    }

    pub fn merge(&self, other: &LangString) -> Self {
        Self {
            en: self.en.as_ref().or(other.en.as_ref()).cloned(),
            ja: self.ja.as_ref().or(other.ja.as_ref()).cloned(),
        }
    }

    pub fn new(lang: Language, value: CachedString) -> Self {
        match lang {
            Language::English => Self {
                en: Some(value),
                ja: None,
            },
            Language::Japanese => Self {
                en: None,
                ja: Some(value),
            },
        }
    }
}

#[derive(Debug)]
pub struct AtomicDateTime(AtomicI64);
impl AtomicDateTime {
    pub fn replace(&self, value: &DateTime) {
        self.0.store(value.timestamp_millis(), Relaxed)
    }

    pub fn as_datetime(&self) -> DateTime {
        Utc.timestamp_millis(self.0.load(Relaxed))
    }

    pub fn as_i64(&self) -> i64 {
        self.0.load(Relaxed)
    }
}

impl Ord for AtomicDateTime {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_i64().cmp(&other.as_i64())
    }
}

impl PartialOrd for AtomicDateTime {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.as_i64().partial_cmp(&other.as_i64())
    }
}

impl Clone for AtomicDateTime {
    fn clone(&self) -> Self {
        AtomicDateTime(AtomicI64::new(self.as_i64()))
    }
}

impl From<&DateTime> for AtomicDateTime {
    fn from(value: &DateTime) -> AtomicDateTime {
        AtomicDateTime(AtomicI64::new(value.timestamp_millis()))
    }
}

impl PartialEq for AtomicDateTime {
    fn eq(&self, other: &Self) -> bool {
        self.as_i64() == other.as_i64()
    }
}

impl Eq for AtomicDateTime {}

static REGEX_LEVEL: Lazy<Regex> =
    Lazy::new(|| Regex::new("^Lv(?:l )?(?P<level>[0-9]+) ").expect("invalid level regex"));

fn parse_level(name: &str) -> Option<Level> {
    REGEX_LEVEL
        .captures(name)
        .and_then(|c| c.name("level"))
        .map(|level| {
            level
                .as_str()
                .parse()
                .expect("Somehow failed to parse level")
        })
}

impl From<&Raid> for Boss {
    fn from(raid: &Raid) -> Self {
        let lang = raid.language;

        let image = match raid.image_url {
            None => LangString::empty(),
            Some(ref url) => LangString::new(lang, url.clone()),
        };

        Self {
            image,
            level: parse_level(&raid.boss_name),
            name: LangString::new(lang, raid.boss_name.clone()),
            last_seen_at: (&raid.created_at).into(),
        }
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn parse_level() {
        assert_eq!(super::parse_level("Lv75 セレスト・マグナ").unwrap(), 75);
        assert_eq!(super::parse_level("Lvl 75 Celeste Omega").unwrap(), 75);
    }
}
