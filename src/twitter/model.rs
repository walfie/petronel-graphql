use crate::model::{CachedString, DateTime, TweetId};
use serde::de::{SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::fmt;

#[derive(Deserialize, PartialEq, Debug)]
pub struct Tweet {
    pub id: TweetId,
    #[serde(deserialize_with = "deserialize_datetime")]
    pub created_at: DateTime,
    pub text: String,
    pub user: User,
    pub entities: Entities,
    pub source: String,
}

#[derive(Deserialize, PartialEq, Debug)]
pub struct Entities {
    // In most cases there should only be one item in the `media` array.
    // We can avoid allocating a `Vec` by using a custom deserializer
    // that only cares about one of the media items.
    #[serde(default, deserialize_with = "deserialize_media")]
    pub media: Option<Media>,
}

#[derive(Deserialize, PartialEq, Debug)]
pub struct Media {
    pub media_url_https: CachedString,
}

#[derive(Deserialize, PartialEq, Debug)]
pub struct User {
    pub id: u64,
    pub screen_name: String,
    pub default_profile_image: bool,
    pub profile_image_url_https: String,
}

fn deserialize_media<'de, D>(deserializer: D) -> Result<Option<Media>, D::Error>
where
    D: Deserializer<'de>,
{
    struct MediaVisitor;

    impl<'de> Visitor<'de> for MediaVisitor {
        type Value = Option<Media>;

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

    deserializer.deserialize_seq(MediaVisitor)
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

#[cfg(test)]
mod test {
    use super::*;
    use chrono::offset::TimeZone;

    #[test]
    fn parse() -> anyhow::Result<()> {
        let input = include_str!("../../tests/tweet.json");

        let parsed = serde_json::from_str::<Tweet>(input)?;

        let expected = Tweet {
            id: 1240718100460273665,
            created_at: chrono::Utc.ymd(2020, 5, 9).and_hms(22, 55, 25),
            text: "457BAF34 :参戦ID\n\
                   参加者募集！\n\
                   Lv120 フラム＝グラス\n\
                   https://t.co/sKmVIG5EdK".to_owned(),
            user: User {
                id: 2955297975,
                screen_name: "walfieee".to_owned(),
                default_profile_image: false,
                profile_image_url_https:
                    "https://abs.twimg.com/sticky/default_profile_images/default_profile_normal.png"
                        .to_owned(),
            },
            entities: Entities {
                media: Some(Media {
                    media_url_https: "https://pbs.twimg.com/media/CVL2EBHUwAA8nUj.jpg".into(),
                }),
            },
            source:
                "<a href=\"http://granbluefantasy.jp/\" rel=\"nofollow\">グランブルー ファンタジー</a>"
                    .into(),
        };

        assert_eq!(parsed, expected);

        Ok(())
    }
}
