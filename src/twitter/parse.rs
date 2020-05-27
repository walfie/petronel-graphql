use crate::model::{Language, Raid};
use crate::twitter::model::Tweet;
use once_cell::sync::Lazy;
use regex::Regex;
use std::borrow::Cow;
use std::convert::TryFrom;

#[derive(Clone, Debug, PartialEq)]
struct TextParts<'a> {
    language: Language,
    text: Option<Cow<'a, str>>,
    raid_id: &'a str,
    boss_name: Cow<'a, str>,
}

#[cfg(test)]
impl<'a> TextParts<'a> {
    fn new(
        language: Language,
        text: Option<&'a str>,
        raid_id: &'a str,
        boss_name: &'a str,
    ) -> Self {
        TextParts {
            language,
            text: text.map(Cow::from),
            raid_id,
            boss_name: boss_name.into(),
        }
    }
}

const GRANBLUE_APP_SOURCE: &'static str =
    r#"<a href="http://granbluefantasy.jp/" rel="nofollow">グランブルー ファンタジー</a>"#;

static REGEX_JAPANESE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        "\
        (?P<text>(?s).*)(?P<id>[0-9A-F]{8}) :参戦ID\n\
        参加者募集！\n\
        (?P<boss>.+)\n?\
        (?P<url>.*)\
    ",
    )
    .expect("invalid Japanese raid tweet regex")
});

static REGEX_ENGLISH: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        "\
        (?P<text>(?s).*)(?P<id>[0-9A-F]{8}) :Battle ID\n\
        I need backup!\n\
        (?P<boss>.+)\n?\
        (?P<url>.*)\
    ",
    )
    .expect("invalid English raid tweet regex")
});

static REGEX_IMAGE_URL: Lazy<Regex> =
    Lazy::new(|| Regex::new("^https?://[^ ]+$").expect("invalid image URL regex"));

fn parse_text<'a>(tweet_text: &'a str) -> Option<TextParts<'a>> {
    REGEX_JAPANESE
        .captures(tweet_text)
        .map(|c| (Language::Japanese, c))
        .or_else(|| {
            REGEX_ENGLISH
                .captures(tweet_text)
                .map(|c| (Language::English, c))
        })
        .and_then(|(lang, c)| {
            if let (Some(text), Some(id), Some(boss), Some(url)) =
                (c.name("text"), c.name("id"), c.name("boss"), c.name("url"))
            {
                let boss_name_raw = boss.as_str().trim();
                let url_str = url.as_str();

                if boss_name_raw.contains("http")
                    || !url_str.is_empty() && !REGEX_IMAGE_URL.is_match(url_str)
                {
                    return None;
                }

                let boss_name = html_decode(boss_name_raw);
                let t = text.as_str().trim();

                Some(TextParts {
                    language: lang,
                    text: if t.is_empty() {
                        None
                    } else {
                        Some(html_decode(t))
                    },
                    raid_id: id.as_str().trim(),
                    boss_name,
                })
            } else {
                None
            }
        })
}

fn html_decode(text: &str) -> Cow<'_, str> {
    if text.contains('&') {
        match escaper::decode_html_sloppy(text) {
            Ok(decoded) => Cow::Owned(decoded),
            _ => Cow::Borrowed(text),
        }
    } else {
        Cow::Borrowed(text)
    }
}

impl TryFrom<Tweet> for Raid {
    type Error = ();

    fn try_from(mut tweet: Tweet) -> Result<Raid, Self::Error> {
        if tweet.source != GRANBLUE_APP_SOURCE {
            return Err(());
        }

        let text = std::mem::replace(&mut tweet.text, String::new());

        let parsed = match parse_text(&text) {
            None => return Err(()),
            Some(parsed) => parsed,
        };

        let user_image = if tweet.user.default_profile_image
            || tweet
                .user
                .profile_image_url_https
                .contains("default_profile")
        {
            None
        } else {
            Some(tweet.user.profile_image_url_https.into())
        };

        let raid = Raid {
            id: parsed.raid_id.to_owned(),
            tweet_id: tweet.id,
            boss_name: parsed.boss_name.into(),
            user_name: tweet.user.screen_name.into(),
            user_image,
            text: parsed.text.map(Cow::into_owned),
            created_at: tweet.created_at.into(),
            language: parsed.language,
            image_url: tweet.entities.media.map(|media| media.media_url_https),
        };

        Ok(raid)
    }
}

#[cfg(test)]
mod test {
    use super::Language::{English, Japanese};
    use super::*;

    #[test]
    fn ignore_invalid_text() {
        assert_eq!(
            parse_text("#GranblueHaiku http://example.com/haiku.png"),
            None
        );
    }

    #[test]
    fn ignore_daily_refresh() {
        // Ignore tweets made via the daily Twitter refresh
        // https://github.com/walfie/gbf-raidfinder/issues/98
        assert_eq!(
            parse_text(
                "救援依頼 参加者募集！参戦ID：114514810\n\
                 Lv100 ケルベロス スマホRPGは今これをやってるよ。\
                 今の推しキャラはこちら！　\
                 ゲーム内プロフィール→　\
                 https://t.co/5Xgohi9wlE https://t.co/Xlu7lqQ3km",
            ),
            None
        );
    }

    #[test]
    fn ignore_another_daily_refresh() {
        // First two lines are user input
        assert_eq!(
            parse_text(
                "救援依頼 参加者募集！参戦ID：114514810\n\
                 Lv100 ケルベロス\n\
                 スマホRPGは今これをやってるよ。\
                 今の推しキャラはこちら！　\
                 ゲーム内プロフィール→　\
                 https://t.co/5Xgohi9wlE https://t.co/Xlu7lqQ3km",
            ),
            None
        );
    }

    #[test]
    fn ignore_extra_space_in_image_url() {
        // First two lines are user input
        assert_eq!(
            parse_text(
                "救援依頼 参加者募集！参戦ID：114514810\n\
                 Lv100 ケルベロス\n\
                 https://t.co/5Xgohi9wlE https://t.co/Xlu7lqQ3km",
            ),
            None
        );
    }

    #[test]
    fn without_extra_text() {
        assert_eq!(
            parse_text(
                "ABCD1234 :参戦ID\n\
                 参加者募集！\n\
                 Lv60 オオゾラッコ\n\
                 http://example.com/image-that-is-ignored.png",
            ),
            Some(TextParts::new(
                Japanese,
                None,
                "ABCD1234",
                "Lv60 オオゾラッコ",
            ))
        );

        assert_eq!(
            parse_text(
                "ABCD1234 :Battle ID\n\
                 I need backup!\n\
                 Lvl 60 Ozorotter\n\
                 http://example.com/image-that-is-ignored.png",
            ),
            Some(TextParts::new(
                English,
                None,
                "ABCD1234",
                "Lvl 60 Ozorotter",
            ))
        );
    }

    #[test]
    fn without_image_url() {
        assert_eq!(
            parse_text(
                "Help me ABCD1234 :参戦ID\n\
                 参加者募集！\n\
                 Lv60 オオゾラッコ",
            ),
            Some(TextParts::new(
                Japanese,
                Some("Help me"),
                "ABCD1234",
                "Lv60 オオゾラッコ",
            ))
        );

        assert_eq!(
            parse_text(
                "Help me ABCD1234 :Battle ID\n\
                 I need backup!\n\
                 Lvl 60 Ozorotter",
            ),
            Some(TextParts::new(
                English,
                Some("Help me"),
                "ABCD1234",
                "Lvl 60 Ozorotter",
            ))
        );
    }

    #[test]
    fn with_extra_newline() {
        assert_eq!(
            parse_text(
                "ABCD1234 :参戦ID\n\
                 参加者募集！\n\
                 Lv60 オオゾラッコ\n",
            ),
            Some(TextParts::new(
                Japanese,
                None,
                "ABCD1234",
                "Lv60 オオゾラッコ",
            ))
        );

        assert_eq!(
            parse_text(
                "ABCD1234 :Battle ID\n\
                 I need backup!\n\
                 Lvl 60 Ozorotter\n",
            ),
            Some(TextParts::new(
                English,
                None,
                "ABCD1234",
                "Lvl 60 Ozorotter",
            ))
        );
    }

    #[test]
    fn extra_text() {
        assert_eq!(
            parse_text(
                "Help me ABCD1234 :参戦ID\n\
                 参加者募集！\n\
                 Lv60 オオゾラッコ\n\
                 http://example.com/image-that-is-ignored.png",
            ),
            Some(TextParts::new(
                Japanese,
                Some("Help me"),
                "ABCD1234",
                "Lv60 オオゾラッコ",
            ))
        );

        assert_eq!(
            parse_text(
                "Help me ABCD1234 :Battle ID\n\
                 I need backup!\n\
                 Lvl 60 Ozorotter\n\
                 http://example.com/image-that-is-ignored.png",
            ),
            Some(TextParts::new(
                English,
                Some("Help me"),
                "ABCD1234",
                "Lvl 60 Ozorotter",
            ))
        );
    }

    #[test]
    fn newlines_in_extra_text() {
        assert_eq!(
            parse_text(
                "Hey\n\
                 Newlines\n\
                 Are\n\
                 Cool\n\
                 ABCD1234 :参戦ID\n\
                 参加者募集！\n\
                 Lv60 オオゾラッコ\n\
                 http://example.com/image-that-is-ignored.png",
            ),
            Some(TextParts::new(
                Japanese,
                Some("Hey\nNewlines\nAre\nCool"),
                "ABCD1234",
                "Lv60 オオゾラッコ",
            ))
        );

        assert_eq!(
            parse_text(
                "Hey\n\
                 Newlines\n\
                 Are\n\
                 Cool\n\
                 ABCD1234 :Battle ID\n\
                 I need backup!\n\
                 Lvl 60 Ozorotter\n\
                 http://example.com/image-that-is-ignored.png",
            ),
            Some(TextParts::new(
                English,
                Some("Hey\nNewlines\nAre\nCool"),
                "ABCD1234",
                "Lvl 60 Ozorotter",
            ))
        );
    }

    #[test]
    fn decode_html_entities() {
        assert_eq!(
            parse_text(
                "Help me ABCD1234 :Battle ID\n\
                 I need backup!\n\
                 Huanglong &amp; Qilin (Impossible)\n\
                 http://example.com/image-that-is-ignored.png",
            ),
            Some(TextParts::new(
                English,
                Some("Help me"),
                "ABCD1234",
                "Huanglong & Qilin (Impossible)",
            ))
        );
    }
}
