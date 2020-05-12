use once_cell::sync::Lazy;
use regex::Regex;

pub type CachedString = string_cache::DefaultAtom;
pub type BossName = CachedString;
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

#[derive(Clone, Debug, PartialEq)]
pub struct Boss {
    pub name: LangString,
    pub image: LangString,
    pub level: Option<Level>,
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
