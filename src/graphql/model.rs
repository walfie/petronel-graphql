use std::sync::Arc;

use crate::model::*;
use crate::RaidHandler;

use async_graphql as gql;
use async_graphql::Context;

pub struct QueryRoot;

#[gql::Object]
impl QueryRoot {
    async fn bosses(&self, ctx: &Context<'_>) -> Vec<Arc<Boss>> {
        ctx.data::<RaidHandler>().bosses()
    }
}

#[gql::Object]
impl LangString {
    #[field(name = "default")]
    async fn gql_default(&self) -> Option<&str> {
        self.ja
            .as_ref()
            .or_else(|| self.en.as_ref())
            .map(AsRef::as_ref)
    }

    async fn ja(&self) -> Option<&str> {
        self.ja.as_ref().map(AsRef::as_ref)
    }

    async fn en(&self) -> Option<&str> {
        self.en.as_ref().map(AsRef::as_ref)
    }
}

#[gql::Object]
impl Boss {
    async fn name(&self) -> &LangString {
        &self.name
    }

    async fn image(&self) -> &LangString {
        &self.image
    }

    async fn level(&self) -> Option<Level> {
        self.level
    }

    async fn tweets(&self, ctx: &gql::Context<'_>) -> Vec<Arc<Raid>> {
        match self.name.default() {
            Some(name) => ctx.data::<RaidHandler>().history(name.clone()),
            None => Vec::new(),
        }
    }
}

#[gql::Object]
impl Raid {
    async fn id(&self) -> &str {
        &self.id
    }

    #[field(name = "tweetId")]
    async fn tweet_id(&self) -> TweetId {
        self.tweet_id
    }

    async fn text(&self) -> &str {
        self.text.as_deref().unwrap_or("")
    }

    #[field(name = "createdAt")]
    async fn created_at(&self) -> DateTime {
        self.created_at
    }

    // TODO: Rename field to username
    #[field(name = "username")]
    async fn user_name(&self) -> &str {
        &self.user_name
    }

    #[field(name = "icon")]
    async fn user_image(&self) -> Option<&str> {
        self.user_image.as_deref()
    }
}
