use crate::{
    cli::{Period, SearchSort, Source, SubSort, ThreadSort, UserSort},
    error::{AppError, ErrorCode},
    model::{Comment, Item, Post, Subreddit, Thread, User, UserSummary},
};

pub use crate::ids::{CommentId, PostId, SubName, UserName};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceKind {
    Live,
    Archive,
}

impl SourceKind {
    pub const fn select(source: Source, has_credentials: bool) -> Self {
        match source {
            Source::Live => Self::Live,
            Source::Archive => Self::Archive,
            Source::Auto if has_credentials => Self::Live,
            Source::Auto => Self::Archive,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Archive => "archive",
        }
    }

    pub const fn alternative(self) -> &'static str {
        match self {
            Self::Live => "--source archive",
            Self::Archive => "--source live",
        }
    }

    pub fn unsupported(self, feature: impl std::fmt::Display) -> AppError {
        AppError::new(
            ErrorCode::EUnsupported,
            format!("{feature} is unsupported with --source {}", self.as_str()),
        )
        .with_hint(format!("Try {}.", self.alternative()))
    }

    pub fn cursor(self, after: Option<String>) -> Result<Option<String>, AppError> {
        let valid = after.as_deref().is_none_or(|cursor| match self {
            // Reddit listing cursors use the fullname of the last thing on the page.
            Self::Live => cursor.split_once('_').is_some_and(|(kind, id)| {
                matches!(kind, "t1" | "t2" | "t3" | "t5") && PostId::parse(id).is_ok()
            }),
            Self::Archive => archive_cursor_key(cursor).is_some(),
        });
        if valid {
            Ok(after)
        } else {
            Err(AppError::usage(format!(
                "invalid cursor for --source {}",
                self.as_str()
            )))
        }
    }
}

pub(crate) fn archive_cursor_key(cursor: &str) -> Option<(i64, &str)> {
    let (created, id) = cursor.strip_prefix("a:")?.split_once(':')?;
    let parsed = PostId::parse(id).ok()?;
    (parsed.as_str() == id).then_some((created.parse().ok()?, id))
}

#[derive(Debug)]
pub struct SubQuery {
    pub name: SubName,
    pub sort: SubSort,
    pub time: Option<Period>,
    pub limit: usize,
    pub after: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SearchQuery {
    pub query: String,
    pub sub: Option<SubName>,
    pub sort: SearchSort,
    pub time: Period,
    pub author: Option<UserName>,
    pub since: Option<i64>,
    pub until: Option<i64>,
    pub limit: usize,
    pub after: Option<String>,
}

#[derive(Debug)]
pub struct ThreadQuery {
    pub post: PostId,
    pub comment: Option<CommentId>,
    pub sort: ThreadSort,
    pub depth: Option<usize>,
    pub limit: usize,
    pub full: bool,
    pub max_requests: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UserListing {
    Overview,
    Posts,
    Comments,
}

#[derive(Debug)]
pub struct UserQuery {
    pub name: UserName,
    pub listing: UserListing,
    pub sort: UserSort,
    pub time: Period,
    pub since: Option<i64>,
    pub until: Option<i64>,
    pub limit: usize,
    pub after: Option<String>,
}

#[derive(Debug)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub after: Option<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Default)]
pub struct ThreadMeta {
    pub more_count: Option<usize>,
    pub requests: Option<usize>,
    pub notes: Vec<String>,
}

/// Adapters return normalized domain records. The command layer stores and renders them.
pub trait Backend: Sync {
    fn kind(&self) -> SourceKind;
    fn subreddit_posts(&self, q: &SubQuery) -> Result<Page<Post>, AppError>;
    fn subreddit_about(&self, name: &SubName) -> Result<Subreddit, AppError>;
    fn search_posts(&self, q: &SearchQuery) -> Result<Page<Post>, AppError>;
    fn search_comments(&self, q: &SearchQuery) -> Result<Page<Comment>, AppError>;
    fn search_subreddits(&self, q: &SearchQuery) -> Result<Page<Subreddit>, AppError>;
    fn search_users(&self, q: &SearchQuery) -> Result<Page<UserSummary>, AppError>;
    fn top_comments(&self, post: &PostId, k: usize) -> Result<Page<Comment>, AppError>;
    fn thread(&self, q: &ThreadQuery) -> Result<(Thread, ThreadMeta), AppError>;
    fn user_about(&self, name: &UserName) -> Result<User, AppError>;
    fn user_items(&self, q: &UserQuery) -> Result<Page<Item>, AppError>;
}
