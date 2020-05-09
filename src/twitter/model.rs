use crate::model::{DateTime, TweetId};

use serde::de::{SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::fmt;

#[derive(Deserialize, Debug)]
pub struct Tweet {
    pub id: TweetId,
    pub created_at: DateTime,
    pub text: String,
    pub user: User,
    pub entities: Entities,
    pub source: String,
}

#[derive(Deserialize, Debug)]
pub struct Entities {
    // In most cases there should only be one item in the `urls` array.
    // We can avoid allocating a `Vec` by using a custom deserializer
    // that only cares about one of the URLs.
    #[serde(default, deserialize_with = "deserialize_url")]
    pub urls: Option<Url>,
}

#[derive(Deserialize, Debug)]
pub struct Url {
    pub expanded_url: String,
}

#[derive(Deserialize, Debug)]
pub struct User {
    pub id: u64,
    pub screen_name: String,
    pub default_profile_image: bool,
    pub profile_image_url_https: String,
}

fn deserialize_url<'de, D>(deserializer: D) -> Result<Option<Url>, D::Error>
where
    D: Deserializer<'de>,
{
    struct UrlVisitor;

    impl<'de> Visitor<'de> for UrlVisitor {
        type Value = Option<Url>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("an array of URL objects")
        }

        fn visit_seq<S>(self, mut seq: S) -> Result<Self::Value, S::Error>
        where
            S: SeqAccess<'de>,
        {
            let mut out = None;

            while let Ok(Some(item)) = seq.next_element() {
                out = item;
            }

            Ok(out)
        }
    }

    deserializer.deserialize_seq(UrlVisitor)
}
