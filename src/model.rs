use once_cell::sync::Lazy;
use regex::Regex;

pub type CachedString = string_cache::DefaultAtom;
pub type DateTime = chrono::DateTime<chrono::Utc>;
pub type Level = i16;
pub type TweetId = u64;
pub type RaidId = String;

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
    fn empty() -> Self {
        Self { en: None, ja: None }
    }

    fn new(lang: Language, value: CachedString) -> Self {
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

#[derive(Clone, Debug, PartialEq)]
pub struct Boss {
    pub name: LangString,
    pub image: LangString,
    pub last_seen: DateTime,
    pub level: Option<Level>,
    // TODO: Image hash
}

#[derive(Clone, Debug, PartialEq)]
pub struct Raid {
    pub id: RaidId,
    pub tweet_id: TweetId,
    pub user_name: String,
    pub user_image: Option<String>,
    pub boss_name: CachedString,
    pub created_at: DateTime,
    pub text: Option<String>,
    pub language: Language,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RaidWithImage {
    pub raid: Raid,
    pub image_url: Option<CachedString>,
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

impl From<RaidWithImage> for Boss {
    fn from(r: RaidWithImage) -> Self {
        let lang = r.raid.language;

        let image = match r.image_url {
            None => LangString::empty(),
            Some(url) => LangString::new(lang, url),
        };

        Self {
            image,
            last_seen: r.raid.created_at,
            level: parse_level(&r.raid.boss_name),
            name: LangString::new(lang, r.raid.boss_name),
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
