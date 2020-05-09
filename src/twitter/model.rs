use crate::model::{DateTime, TweetId};

use serde::de::{SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::fmt;

#[derive(Deserialize, Debug)]
pub struct Tweet {
    pub id: TweetId,
    #[serde(deserialize_with = "deserialize_datetime")]
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

// Based heavily on the deserializer from the `twitter-stream-message` crate
fn deserialize_datetime<'de, D>(deserializer: D) -> Result<DateTime, D::Error>
where
    D: Deserializer<'de>,
{
    struct DateTimeVisitor;

    impl<'de> Visitor<'de> for DateTimeVisitor {
        type Value = DateTime;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid date time string")
        }

        fn visit_str<E>(self, s: &str) -> Result<DateTime, E>
        where
            E: serde::de::Error,
        {
            pub fn parse_datetime(s: &str) -> ::chrono::format::ParseResult<DateTime> {
                use chrono::format::{self, Fixed, Item, Numeric, Pad, Parsed};
                use chrono::Utc;

                // "%a %b %e %H:%M:%S %z %Y"
                const ITEMS: &'static [Item<'static>] = &[
                    Item::Fixed(Fixed::ShortWeekdayName),
                    Item::Space(" "),
                    Item::Fixed(Fixed::ShortMonthName),
                    Item::Space(" "),
                    Item::Numeric(Numeric::Day, Pad::Space),
                    Item::Space(" "),
                    Item::Numeric(Numeric::Hour, Pad::Zero),
                    Item::Literal(":"),
                    Item::Numeric(Numeric::Minute, Pad::Zero),
                    Item::Literal(":"),
                    Item::Numeric(Numeric::Second, Pad::Zero),
                    Item::Space(" "),
                    Item::Fixed(Fixed::TimezoneOffset),
                    Item::Space(" "),
                    Item::Numeric(Numeric::Year, Pad::Zero),
                ];

                let mut parsed = Parsed::new();
                format::parse(&mut parsed, s, ITEMS.iter().cloned())?;
                parsed.to_datetime_with_timezone(&Utc)
            }

            parse_datetime(s).map_err(|e| E::custom(e.to_string()))
        }
    }

    deserializer.deserialize_str(DateTimeVisitor)
}
