use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

pub fn timestamp(seconds: i64) -> String {
    chrono::DateTime::from_timestamp(seconds, 0)
        .map(|dt| dt.to_rfc3339_opts(SecondsFormat::Secs, true))
        .unwrap_or_default()
}

pub fn now() -> i64 {
    Utc::now().timestamp()
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    #[default]
    Ok,
    Deleted,
    Removed,
}

impl State {
    pub const fn marker(self) -> Option<&'static str> {
        match self {
            Self::Ok => None,
            Self::Deleted => Some("[deleted]"),
            Self::Removed => Some("[removed]"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaType {
    #[default]
    None,
    Image,
    Gallery,
    Video,
    Link,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Media {
    #[serde(rename = "type")]
    pub kind: MediaType,
    pub urls: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(tag = "kind", rename = "post")]
pub struct Post {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_comments: Option<Vec<Comment>>,
    pub id: String,
    pub fullname: String,
    pub subreddit: String,
    pub author: String,
    pub title: String,
    pub selftext: String,
    pub url: String,
    pub domain: String,
    pub permalink: String,
    pub created_utc: i64,
    pub created: String,
    pub score: Option<i64>,
    pub upvote_ratio: Option<f64>,
    pub num_comments: Option<i64>,
    pub over_18: bool,
    pub spoiler: bool,
    pub stickied: bool,
    pub locked: bool,
    pub is_self: bool,
    pub flair: Option<String>,
    pub crosspost_parent: Option<String>,
    pub media: Media,
    pub state: State,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(tag = "kind", rename = "comment")]
pub struct Comment {
    pub id: String,
    pub fullname: String,
    pub link_id: String,
    pub parent_id: String,
    pub subreddit: String,
    pub author: String,
    pub body: String,
    pub created_utc: i64,
    pub created: String,
    pub score: Option<i64>,
    pub depth: usize,
    pub is_submitter: bool,
    pub stickied: bool,
    pub distinguished: Option<String>,
    pub edited: bool,
    pub state: State,
    pub permalink: String,
    pub replies: Vec<Comment>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Thread {
    pub post: Post,
    pub comments: Vec<Comment>,
    pub complete: bool,
    pub unresolved: Vec<Unresolved>,
    pub comments_returned: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Unresolved {
    pub id: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Subreddit {
    pub name: String,
    pub title: String,
    pub description: String,
    pub subscribers: Option<i64>,
    pub active_users: Option<i64>,
    pub created_utc: i64,
    pub created: String,
    pub over_18: bool,
    pub url: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct User {
    pub name: String,
    pub created_utc: i64,
    pub created: String,
    pub link_karma: i64,
    pub comment_karma: i64,
    pub total_karma: i64,
    pub is_suspended: bool,
    pub is_employee: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct UserSummary {
    pub name: String,
    pub created_utc: i64,
    pub created: String,
    pub total_karma: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Item {
    Post(Post),
    Comment(Comment),
}

#[derive(Debug, Serialize)]
pub struct Meta {
    pub source: &'static str,
    pub cached: bool,
    pub fetched_at: String,
    pub after: Option<String>,
    pub notes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub more_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requests: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<crate::store::RateLimit>,
}

impl Meta {
    pub fn new(source: &'static str) -> Self {
        Self {
            source,
            cached: false,
            fetched_at: timestamp(now()),
            after: None,
            notes: Vec::new(),
            more_count: None,
            requests: None,
            rate_limit: None,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Envelope<'a, T> {
    pub data: T,
    pub meta: &'a Meta,
}
