pub type CachedString = string_cache::DefaultAtom;
pub type DateTime = chrono::DateTime<chrono::Utc>;
pub type Level = i16;
pub type TweetId = u64;
pub type RaidId = String;

#[derive(Clone, Debug, PartialEq)]
pub enum Language {
    Japanese,
    English,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LangString {
    pub en: Option<CachedString>,
    pub ja: Option<CachedString>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Boss {
    pub name: LangString,
    pub image: LangString,
    pub last_seen: DateTime,
    pub level: Level,
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
