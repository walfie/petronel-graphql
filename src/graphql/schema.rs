use std::borrow::Cow;
use std::str;
use std::sync::Arc;

use crate::model::*;
use crate::raid_handler::{BossEntry, RaidHandler};

use async_graphql as gql;
use async_graphql::Context;
use futures::stream::Stream;

#[gql::Interface(field(name = "id", type = "gql::ID"))]
enum Node {
    Boss(Arc<BossEntry>),
    Tweet(Arc<Raid>),
}

pub struct QueryRoot;

#[gql::Object]
impl QueryRoot {
    async fn node(&self, ctx: &Context<'_>, id: gql::ID) -> Option<Node> {
        match id.parse().ok()? {
            NodeId::Boss(name) => ctx.data::<RaidHandler>().boss(&name).map(Node::Boss),
            NodeId::Tweet { boss_name, id } => {
                ctx.data::<RaidHandler>().boss(&boss_name).and_then(|boss| {
                    boss.history()
                        .read()
                        .iter()
                        .find(|tweet| tweet.tweet_id == id)
                        .map(|t| Node::Tweet(t.clone()))
                })
            }
        }
    }

    // TODO: pagination, first/last, etc
    async fn bosses(&self, ctx: &Context<'_>) -> Vec<Arc<BossEntry>> {
        ctx.data::<RaidHandler>().bosses().clone()
    }

    async fn boss(&self, ctx: &Context<'_>, name: String) -> Option<Arc<BossEntry>> {
        ctx.data::<RaidHandler>().boss(&name.into())
    }
}

pub struct SubscriptionRoot;

#[gql::Subscription]
impl SubscriptionRoot {
    async fn bosses(&self, ctx: &Context<'_>) -> impl Stream<Item = Arc<BossEntry>> {
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

#[gql::Object]
/// A string (name, URL, etc) that differs based on language
impl LangString {
    /// The Japanese string, if it exists. Otherwise, the English one.
    #[field(name = "canonical")]
    async fn gql_canonical(&self) -> Option<&str> {
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

#[gql::Object(name = "Boss")]
/// A raid boss
impl BossEntry {
    /// Node ID
    async fn id(&self) -> gql::ID {
        self.node_id().into()
    }

    /// Boss name
    async fn name(&self) -> &LangString {
        &self.boss().name
    }

    /// Twitter image URL
    async fn image(&self) -> &LangString {
        &self.boss().image
    }

    /// The level of the boss, if known
    async fn level(&self) -> Option<Level> {
        self.boss().level
    }

    /// List of raid tweets
    async fn tweets(&self) -> Vec<Arc<Raid>> {
        // TODO: Pagination
        self.history().read().iter().cloned().collect()
    }
}

#[gql::Object(name = "Tweet")]
/// A tweet containing a raid invite
impl Raid {
    /// Node ID
    async fn id(&self) -> gql::ID {
        let node_id = NodeId::Tweet {
            id: self.tweet_id,
            boss_name: Cow::Borrowed(&self.boss_name),
        };

        node_id.to_string().into()
    }

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

    // TODO: Add scalar type for DateTime as string
    /// Tweet creation date
    #[field(name = "createdAt")]
    async fn created_at(&self) -> &str {
        self.created_at.as_str()
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
