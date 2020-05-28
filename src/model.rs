use chrono::offset::{TimeZone, Utc};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::cmp::Ordering;
use std::str;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering::Relaxed;

pub use crate::image_hash::phash::ImageHash;
pub type CachedString = string_cache::DefaultAtom;
pub type BossName = CachedString;
pub type DateTime = chrono::DateTime<Utc>;
pub type Level = i16;
pub type TweetId = u64;
pub type RaidId = String;

// GraphQL Node ID
#[derive(Debug, Clone, PartialEq)]
pub enum NodeId {
    Boss(BossName),
}

impl ToString for NodeId {
    fn to_string(&self) -> String {
        let string_to_encode = match self {
            Self::Boss(name) => format!("boss:{}", name),
        };
        bs58::encode(&string_to_encode).into_string()
    }
}

impl str::FromStr for NodeId {
    type Err = ();

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let bytes = bs58::decode(input).into_vec().map_err(|_| ())?;
        let decoded = str::from_utf8(&bytes).map_err(|_| ())?;

        let mut parts = decoded.splitn(2, ':');
        match (parts.next(), parts.next()) {
            (Some("boss"), Some(id)) => Ok(Self::Boss(id.into())),
            _ => Err(()),
        }
    }
}

impl NodeId {
    pub fn from_boss_name(name: &LangString) -> Self {
        // Use whichever boss name is smaller (in terms of bytes),
        // since both names resolve to the same boss anyway
        let shorter_name = Language::VALUES
            .iter()
            .flat_map(|lang| name.get(*lang))
            .min_by_key(|n| n.len())
            .cloned()
            .unwrap_or_else(|| "".into());

        Self::Boss(shorter_name)
    }
}

#[serde(rename_all = "camelCase")]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Boss {
    pub name: LangString,
    pub image: LangString,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<Level>,
    pub last_seen_at: AtomicDateTime,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_hash: Option<ImageHash>,
}

impl Boss {
    // Unfortunately, this has to be hardcoded somewhere because the boss
    // image hashes are different between the English and Japanese versions.
    // https://github.com/walfie/gbf-raidfinder/blob/master/docs/implementation.md#automatic-translations
    pub const LVL_120_MEDUSA: Lazy<Boss> = Lazy::new(|| Boss {
        name: LangString {
            ja: Some("Lv120 メドゥーサ".into()),
            en: Some("Lvl 120 Medusa".into()),
        },
        image: LangString::default(),
        level: Some(120),
        last_seen_at: AtomicDateTime::now(),
        image_hash: None,
    });

    pub fn needs_image_hash_update(&self) -> bool {
        self.image_hash.is_none() && self.image.canonical().is_some()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Raid {
    pub id: RaidId,
    pub tweet_id: TweetId,
    pub user_name: String,
    // TODO: Strip prefix "https://pbs.twimg.com/profile_images/" to reduce size of messages.
    // Include the full URL as a separate graphql field if requested.
    pub user_image: Option<String>,
    pub boss_name: BossName,
    pub created_at: DateTimeString,
    pub text: Option<String>,
    pub language: Language,
    pub image_url: Option<CachedString>,
}

// A premature optimization to avoid needing to stringify a `DateTime` multiple times
#[derive(Debug, Clone)]
pub struct DateTimeString {
    string: String,
    datetime: DateTime,
}

impl DateTimeString {
    pub fn as_str(&self) -> &str {
        &self.string
    }

    pub fn as_datetime(&self) -> &DateTime {
        &self.datetime
    }
}

impl PartialEq for DateTimeString {
    fn eq(&self, other: &Self) -> bool {
        self.as_datetime() == other.as_datetime()
    }
}

impl Eq for DateTimeString {}

impl PartialOrd for DateTimeString {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.as_datetime().partial_cmp(other.as_datetime())
    }
}

impl Ord for DateTimeString {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_datetime().cmp(other.as_datetime())
    }
}

impl From<DateTime> for DateTimeString {
    fn from(value: DateTime) -> Self {
        Self {
            string: value.to_string(),
            datetime: value,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Language {
    Japanese,
    English,
}

impl Language {
    pub const VALUES: &'static [Language] = &[Self::Japanese, Self::English];

    pub fn as_metric_label(&self) -> &'static str {
        match self {
            Self::Japanese => "ja",
            Self::English => "en",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct LangString {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub en: Option<CachedString>,
    #[serde(skip_serializing_if = "Option::is_none")]
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

    pub fn canonical(&self) -> Option<&CachedString> {
        self.ja.as_ref().or_else(|| self.en.as_ref())
    }

    pub fn set(&mut self, lang: Language, value: Option<CachedString>) {
        match lang {
            Language::English => self.en = value,
            Language::Japanese => self.ja = value,
        }
    }

    pub fn for_each(&self, mut f: impl FnMut(&BossName)) {
        for opt in &[self.ja.as_ref(), self.en.as_ref()] {
            if let Some(value) = opt {
                f(value)
            }
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
    pub fn now() -> Self {
        Self::from(&Utc::now())
    }

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

impl From<i64> for AtomicDateTime {
    fn from(value: i64) -> AtomicDateTime {
        AtomicDateTime(AtomicI64::new(value))
    }
}

impl PartialEq for AtomicDateTime {
    fn eq(&self, other: &Self) -> bool {
        self.as_i64() == other.as_i64()
    }
}

impl Eq for AtomicDateTime {}

impl<'de> Deserialize<'de> for AtomicDateTime {
    #[inline]
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let val = i64::deserialize(deserializer)?;
        Ok(val.into())
    }
}

impl Serialize for AtomicDateTime {
    #[inline]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_i64(self.as_i64())
    }
}

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
            None => Default::default(),
            Some(ref url) => LangString::new(lang, url.clone()),
        };

        Self {
            image,
            image_hash: None,
            level: parse_level(&raid.boss_name),
            name: LangString::new(lang, raid.boss_name.clone()),
            last_seen_at: raid.created_at.as_datetime().into(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parse_level() {
        assert_eq!(super::parse_level("Lv75 セレスト・マグナ").unwrap(), 75);
        assert_eq!(super::parse_level("Lvl 75 Celeste Omega").unwrap(), 75);
    }

    #[test]
    fn boss_serde() {
        let json = serde_json::from_str::<Boss>(
            r#"{
                "name": {
                    "en": "Lvl 60 Ozorotter",
                    "ja": "Lv60 オオゾラッコ"
                },
                "image": {
                    "en": "http://example.com/image_en.png",
                    "ja": "http://example.com/image_ja.png"
                },
                "level": 60,
                "lastSeenAt": 1234,
                "imageHash": 6789
            }"#,
        )
        .unwrap();

        let boss = Boss {
            name: LangString {
                en: Some("Lvl 60 Ozorotter".into()),
                ja: Some("Lv60 オオゾラッコ".into()),
            },
            image: LangString {
                en: Some("http://example.com/image_en.png".into()),
                ja: Some("http://example.com/image_ja.png".into()),
            },
            level: Some(60),
            last_seen_at: AtomicDateTime::from(1234),
            image_hash: Some(ImageHash::from(6789)),
        };

        assert_eq!(json, boss);
    }

    #[test]
    fn node_id() {
        let id = NodeId::Boss("Lvl 60 Ozorotter".into());
        assert_eq!(id.to_string().parse::<NodeId>().unwrap(), id);
        assert_eq!(
            "7456rjyoQwqQfRqRH2mGW7W2S67e1".parse::<NodeId>().unwrap(),
            id
        );
    }
}
