pub mod auth;
mod more;
mod parse;

use crate::{
    backend::{
        Backend, Page, PostId, SearchQuery, SourceKind, SubName, SubQuery, ThreadMeta, ThreadQuery,
        UserListing, UserName, UserQuery,
    },
    error::AppError,
    http::{HttpClient, LISTING_TTL, PROFILE_TTL, THREAD_TTL},
    model::{Comment, Item, Post, Subreddit, Thread, User, UserSummary},
    store::Store,
};
use auth::TokenProvider;
use serde_json::Value;
use std::{collections::HashSet, sync::Arc};

const SOURCE: SourceKind = SourceKind::Live;

pub struct LiveBackend {
    http: Arc<HttpClient>,
    tokens: Option<Arc<TokenProvider>>,
}

impl LiveBackend {
    pub fn new(
        http: Arc<HttpClient>,
        _store: Arc<Store>,
        tokens: Option<Arc<TokenProvider>>,
    ) -> Self {
        Self { http, tokens }
    }

    fn get(&self, path: &str, query: &[(&str, &str)], ttl: i64) -> Result<Value, AppError> {
        Ok(self
            .http
            .get(SOURCE, path, query, ttl, self.tokens.as_deref())?
            .body)
    }

    fn listing<T>(
        &self,
        path: &str,
        query: &[(&str, &str)],
        ttl: i64,
        limit: usize,
        mut after: Option<String>,
        parse_thing: impl Fn(&Value) -> Result<T, AppError>,
    ) -> Result<Page<T>, AppError> {
        let mut items = Vec::new();
        let mut cursors = HashSet::new();
        while items.len() < limit {
            let remaining = limit - items.len();
            let request_limit = remaining.min(100).to_string();
            let mut params = query.to_vec();
            params.push(("limit", &request_limit));
            if let Some(cursor) = after.as_deref() {
                cursors.insert(cursor.to_owned());
                params.push(("after", cursor));
            }
            let response = self.get(path, &params, ttl)?;
            let (children, next) = parse::listing(&response)?;
            for thing in children.iter().take(remaining) {
                items.push(parse_thing(thing)?);
            }
            after = next.map(str::to_owned);
            let Some(cursor) = after.as_ref() else {
                break;
            };
            if cursors.contains(cursor) {
                return Err(AppError::upstream("Reddit repeated a listing cursor"));
            }
            if children.is_empty() {
                break;
            }
        }
        Ok(Page {
            items,
            after,
            notes: Vec::new(),
        })
    }

    fn validate_search(q: &SearchQuery) -> Result<(), AppError> {
        if q.author.is_some() || q.since.is_some() || q.until.is_some() {
            return Err(SOURCE.unsupported("--author/--since/--until"));
        }
        Ok(())
    }
}

impl Backend for LiveBackend {
    fn kind(&self) -> SourceKind {
        SOURCE
    }

    fn subreddit_posts(&self, q: &SubQuery) -> Result<Page<Post>, AppError> {
        let path = format!("/r/{}/{}", q.name, q.sort.as_str());
        let params: Vec<_> = q
            .time
            .map(|time| ("t", time.as_str()))
            .into_iter()
            .collect();
        self.listing(
            &path,
            &params,
            LISTING_TTL,
            q.limit,
            q.after.clone(),
            parse::post,
        )
    }

    fn subreddit_about(&self, name: &SubName) -> Result<Subreddit, AppError> {
        parse::subreddit(&self.get(&format!("/r/{name}/about"), &[], PROFILE_TTL)?)
    }

    fn search_posts(&self, q: &SearchQuery) -> Result<Page<Post>, AppError> {
        Self::validate_search(q)?;
        let mut params = vec![
            ("q", q.query.as_str()),
            ("sort", q.sort.as_str()),
            ("t", q.time.as_str()),
        ];
        let path = match &q.sub {
            Some(sub) => {
                params.push(("restrict_sr", "true"));
                format!("/r/{sub}/search")
            }
            None => "/search".to_owned(),
        };
        self.listing(
            &path,
            &params,
            LISTING_TTL,
            q.limit,
            q.after.clone(),
            parse::post,
        )
    }

    fn search_comments(&self, _: &SearchQuery) -> Result<Page<Comment>, AppError> {
        Err(SOURCE.unsupported("--type comments"))
    }

    fn search_subreddits(&self, q: &SearchQuery) -> Result<Page<Subreddit>, AppError> {
        Self::validate_search(q)?;
        self.listing(
            "/subreddits/search",
            &[("q", &q.query)],
            LISTING_TTL,
            q.limit,
            q.after.clone(),
            parse::subreddit,
        )
    }

    fn search_users(&self, q: &SearchQuery) -> Result<Page<UserSummary>, AppError> {
        Self::validate_search(q)?;
        self.listing(
            "/users/search",
            &[("q", &q.query)],
            LISTING_TTL,
            q.limit,
            q.after.clone(),
            parse::user_summary,
        )
    }

    fn top_comments(&self, post: &PostId, k: usize) -> Result<Page<Comment>, AppError> {
        let limit = k.to_string();
        let body = self.get(
            &format!("/comments/{post}"),
            &[("sort", "top"), ("depth", "1"), ("limit", &limit)],
            THREAD_TTL,
        )?;
        let (post, comments) = parse::thread(&body)?;
        Ok(Page {
            items: comments
                .comments
                .into_iter()
                .filter(|comment| comment.parent_id == post.fullname)
                .take(k)
                .map(|mut comment| {
                    comment.depth = 0;
                    comment
                })
                .collect(),
            after: None,
            notes: Vec::new(),
        })
    }

    fn thread(&self, q: &ThreadQuery) -> Result<(Thread, ThreadMeta), AppError> {
        more::thread(self, q)
    }

    fn user_about(&self, name: &UserName) -> Result<User, AppError> {
        parse::user(&self.get(&format!("/user/{name}/about"), &[], PROFILE_TTL)?)
    }

    fn user_items(&self, q: &UserQuery) -> Result<Page<Item>, AppError> {
        if q.since.is_some() || q.until.is_some() {
            return Err(SOURCE.unsupported("--since/--until"));
        }
        let listing = match q.listing {
            UserListing::Overview => "overview",
            UserListing::Posts => "submitted",
            UserListing::Comments => "comments",
        };
        self.listing(
            &format!("/user/{}/{listing}", q.name),
            &[("sort", q.sort.as_str()), ("t", q.time.as_str())],
            PROFILE_TTL,
            q.limit,
            q.after.clone(),
            parse::item,
        )
    }
}
