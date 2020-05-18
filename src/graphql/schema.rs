use std::sync::Arc;

use crate::model::*;
use crate::RaidHandler;

use async_graphql as gql;
use async_graphql::Context;
use futures::stream::Stream;

pub struct SubscriptionRoot;

#[gql::Subscription]
impl SubscriptionRoot {
    async fn bosses(&self, ctx: &Context<'_>) -> impl Stream<Item = Arc<Boss>> {
        ctx.data::<RaidHandler>().subscribe_boss_updates()
    }

    async fn tweets(
        &self,
        ctx: &Context<'_>,
        #[arg(name = "bossName")] boss_name: String,
    ) -> impl Stream<Item = Arc<Raid>> {
        ctx.data::<RaidHandler>().subscribe(boss_name.into())
    }
}

pub struct QueryRoot;

#[gql::Object]
impl QueryRoot {
    async fn bosses(&self, ctx: &Context<'_>) -> Vec<Arc<Boss>> {
        ctx.data::<RaidHandler>().bosses()
    }
}

#[gql::Object]
/// A string (name, URL, etc) that differs based on language
impl LangString {
    /// The Japanese string, if it exists. Otherwise, the English one.
    #[field(name = "default")]
    async fn gql_default(&self) -> Option<&str> {
        self.ja.as_deref().or_else(|| self.en.as_deref())
    }

    /// Japanese string
    async fn ja(&self) -> Option<&str> {
        self.ja.as_deref()
    }

    /// English string
    async fn en(&self) -> Option<&str> {
        self.en.as_deref()
    }
}

#[gql::Object]
/// A raid boss
impl Boss {
    /// Boss name
    async fn name(&self) -> &LangString {
        &self.name
    }

    /// Twitter image URL
    async fn image(&self) -> &LangString {
        &self.image
    }

    /// The level of the boss, if known
    async fn level(&self) -> Option<Level> {
        self.level
    }

    /// List of raid tweets
    async fn tweets(&self, ctx: &gql::Context<'_>) -> Vec<Arc<Raid>> {
        match self.name.default() {
            Some(name) => ctx.data::<RaidHandler>().history(name.clone()),
            None => Vec::new(),
        }
    }
}

#[gql::Object]
/// A tweet containing a raid invite
impl Raid {
    /// Raid ID
    async fn raid_id(&self) -> &str {
        &self.id
    }

    /// Tweet ID
    #[field(name = "tweetId")]
    async fn tweet_id(&self) -> TweetId {
        self.tweet_id
    }

    /// Additional text associated with the tweet
    async fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    /// Tweet creation date
    #[field(name = "createdAt")]
    async fn created_at(&self) -> DateTime {
        self.created_at
    }

    /// Twitter username
    async fn username(&self) -> &str {
        &self.user_name
    }

    /// Twitter user icon
    #[field(name = "icon")]
    async fn user_image(&self) -> Option<&str> {
        self.user_image.as_deref()
    }
}
