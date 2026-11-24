mod parse;

use crate::{
    backend::{
        Backend, Page, PostId, SearchQuery, SourceKind, SubName, SubQuery, ThreadMeta, ThreadQuery,
        UserListing, UserName, UserQuery, archive_cursor_key,
    },
    cli::{Period, SearchSort, SubSort, ThreadSort, UserSort},
    error::{AppError, ErrorCode},
    http::{HttpClient, LISTING_TTL},
    model::{Comment, Item, Post, Subreddit, Thread, User, UserSummary},
    store::Store,
    tree,
};
use serde_json::Value;
use std::{cmp::Reverse, collections::HashSet, sync::Arc};

const SOURCE: SourceKind = SourceKind::Archive;
const PAGE_SIZE: usize = 100;
const COMMENT_PAGES: usize = 5;
const TREE_LIMIT: &str = "9999";
const USER_DATE_NOTE: &str = "Archive user creation dates use the earliest known post or comment when the account creation date is unavailable.";

type Params<'a> = Vec<(&'static str, &'a str)>;
type TimelineRow = ((i64, String), Value);

#[derive(Clone, Copy, Default)]
struct Bounds<'a> {
    since: Option<i64>,
    until: Option<i64>,
    cursor: Option<(i64, &'a str)>,
}

impl<'a> Bounds<'a> {
    fn new(
        since: Option<i64>,
        until: Option<i64>,
        after: Option<&'a str>,
    ) -> Result<Self, AppError> {
        let cursor = after
            .map(|cursor| {
                archive_cursor_key(cursor)
                    .ok_or_else(|| AppError::usage("invalid cursor for --source archive"))
            })
            .transpose()?;
        Ok(Self {
            since,
            until,
            cursor,
        })
    }
}

pub struct ArchiveBackend {
    http: Arc<HttpClient>,
}

impl ArchiveBackend {
    // The command and HTTP layers own item and response persistence respectively.
    pub fn new(http: Arc<HttpClient>, _store: Arc<Store>) -> Self {
        Self { http }
    }

    fn records(&self, endpoint: &str, params: &[(&str, &str)]) -> Result<Vec<Value>, AppError> {
        let mut response = self.http.get(SOURCE, endpoint, params, LISTING_TTL, None)?;
        match response.body.get_mut("data").map(Value::take) {
            Some(Value::Array(rows)) => Ok(rows),
            _ => Err(AppError::upstream(format!(
                "archive {endpoint} did not return a data array"
            ))),
        }
    }

    fn timeline(
        &self,
        endpoint: &str,
        mut params: Params<'_>,
        limit: usize,
        bounds: Bounds<'_>,
        page_cap: Option<usize>,
    ) -> Result<Page<Value>, AppError> {
        let mut items = Vec::new();
        let mut seen = HashSet::new();
        let cursor_end = bounds.cursor.and_then(|(second, _)| second.checked_add(1));
        let mut before = bounds.until.into_iter().chain(cursor_end).min();
        let mut has_more = false;
        params.push(("sort", "desc"));

        let max_pages = page_cap.unwrap_or(usize::MAX);
        for page in 1..=max_pages {
            if matches!((bounds.since, before), (Some(start), Some(end)) if start >= end) {
                break;
            }
            let mut rows = self.timeline_rows(endpoint, &params, bounds.since, before)?;
            let full = rows.len() >= PAGE_SIZE;
            rows.retain(|((created, _), _)| before.is_none_or(|end| *created < end));
            let Some(&oldest) = rows.last().map(|((created, _), _)| created) else {
                has_more = false;
                break;
            };
            let above_boundary = rows.partition_point(|((created, _), _)| *created > oldest);
            if full {
                // Arctic Shift only orders by time. Carry the final second into the
                // next request so its rows can be ordered together by ID.
                if above_boundary > 0 && page < max_pages {
                    rows.truncate(above_boundary);
                } else if page_cap.is_none() {
                    return Err(AppError::upstream(format!(
                        "archive pagination cannot resolve a full page at second {oldest}"
                    )));
                }
            }

            let mut remaining = rows.into_iter().filter(|(key, _)| {
                bounds
                    .cursor
                    .is_none_or(|cursor| (key.0, key.1.as_str()) < cursor)
                    && seen.insert(key.1.clone())
            });
            items.extend(remaining.by_ref().take(limit - items.len()));
            has_more = full || remaining.next().is_some();
            if items.len() == limit || !full || above_boundary == 0 {
                break;
            }
            before = oldest.checked_add(1);
        }

        let after = items
            .last()
            .filter(|_| has_more)
            .map(|((created, id), _)| cursor(*created, id));
        Ok(Page {
            items: items.into_iter().map(|(_, row)| row).collect(),
            after,
            notes: Vec::new(),
        })
    }

    fn timeline_rows(
        &self,
        endpoint: &str,
        params: &[(&str, &str)],
        since: Option<i64>,
        before: Option<i64>,
    ) -> Result<Vec<TimelineRow>, AppError> {
        let mut query = params.to_vec();
        let limit = PAGE_SIZE.to_string();
        let since = since.map(|value| value.to_string());
        let before = before.map(|value| value.to_string());
        query.push(("limit", &limit));
        if let Some(since) = &since {
            query.push(("after", since));
        }
        if let Some(before) = &before {
            query.push(("before", before));
        }
        let mut rows = self
            .records(endpoint, &query)?
            .into_iter()
            .map(|row| {
                let (id, created) = parse::key(&row)?;
                Ok(((created, id.to_owned()), row))
            })
            .collect::<Result<Vec<_>, AppError>>()?;
        rows.sort_unstable_by(|(left, _), (right, _)| right.cmp(left));
        Ok(rows)
    }

    fn search_timeline(
        &self,
        q: &SearchQuery,
        endpoint: &str,
        keyword: &'static str,
    ) -> Result<Page<Value>, AppError> {
        validate_search(q)?;
        if q.sub.is_none() && q.author.is_none() {
            return Err(AppError::usage(
                "archive post and comment search requires --sub or --author",
            ));
        }
        let mut params = vec![(keyword, q.query.as_str())];
        if let Some(sub) = &q.sub {
            single_sub(sub)?;
            params.push(("subreddit", sub.as_str()));
        }
        if let Some(author) = &q.author {
            params.push(("author", author.as_str()));
        }
        self.timeline(
            endpoint,
            params,
            q.limit,
            Bounds::new(q.since, q.until, q.after.as_deref())?,
            None,
        )
    }
}

impl Backend for ArchiveBackend {
    fn kind(&self) -> SourceKind {
        SOURCE
    }

    fn subreddit_posts(&self, q: &SubQuery) -> Result<Page<Post>, AppError> {
        single_sub(&q.name)?;
        if q.sort != SubSort::New {
            return Err(SOURCE.unsupported(format_args!("--sort {}", q.sort.as_str())));
        }
        if q.time.is_some() {
            return Err(SOURCE.unsupported("--time"));
        }
        let bounds = Bounds::new(None, None, q.after.as_deref())?;
        map_page(
            self.timeline(
                "/posts/search",
                vec![("subreddit", q.name.as_str())],
                q.limit,
                bounds,
                None,
            )?,
            parse::post,
        )
    }

    fn subreddit_about(&self, name: &SubName) -> Result<Subreddit, AppError> {
        single_sub(name)?;
        let rows = self.records("/subreddits/search", &[("subreddit", name.as_str())])?;
        parse::subreddit(exact(&rows, "display_name", name.as_str(), "subreddit")?)
    }

    fn search_posts(&self, q: &SearchQuery) -> Result<Page<Post>, AppError> {
        map_page(
            self.search_timeline(q, "/posts/search", "query")?,
            parse::post,
        )
    }

    fn search_comments(&self, q: &SearchQuery) -> Result<Page<Comment>, AppError> {
        map_page(
            self.search_timeline(q, "/comments/search", "body")?,
            parse::comment,
        )
    }

    // Name searches rank by popularity (subscribers / karma), the order a researcher
    // expects from "find the rust subreddit". Arctic Shift cannot resume these orders
    // by timestamp, so they return one page of up to --limit results and no cursor.
    fn search_subreddits(&self, q: &SearchQuery) -> Result<Page<Subreddit>, AppError> {
        validate_search(q)?;
        reject_name_cursor(q)?;
        let limit = q.limit.to_string();
        let rows = self.records(
            "/subreddits/search",
            &[
                ("subreddit_prefix", q.query.as_str()),
                ("sort_type", "subscribers"),
                ("limit", &limit),
            ],
        )?;
        Ok(Page {
            items: rows
                .iter()
                .map(parse::subreddit)
                .collect::<Result<_, _>>()?,
            after: None,
            notes: Vec::new(),
        })
    }

    fn search_users(&self, q: &SearchQuery) -> Result<Page<UserSummary>, AppError> {
        validate_search(q)?;
        reject_name_cursor(q)?;
        let limit = q.limit.to_string();
        let rows = self.records(
            "/users/search",
            &[
                ("author_prefix", q.query.as_str()),
                ("sort_type", "total_karma"),
                ("limit", &limit),
            ],
        )?;
        Ok(Page {
            items: rows
                .iter()
                .map(parse::user_summary)
                .collect::<Result<_, _>>()?,
            after: None,
            notes: vec![USER_DATE_NOTE.into()],
        })
    }

    fn top_comments(&self, post: &PostId, k: usize) -> Result<Page<Comment>, AppError> {
        let link_id = format!("t3_{post}");
        // Arctic Shift orders by time. Rank the fetched roots locally by score.
        let raw = self.timeline(
            "/comments/search",
            vec![("link_id", &link_id), ("parent_id", "")],
            PAGE_SIZE * COMMENT_PAGES,
            Bounds::default(),
            Some(COMMENT_PAGES),
        )?;
        let mut page = map_page(raw, parse::comment)?;
        if page.after.take().is_some() {
            page.notes.push(format!(
                "Post {post}: archive top comments were truncated (up to {COMMENT_PAGES} pages)."
            ));
        }
        page.items.sort_by_key(|comment| Reverse(comment.score));
        page.items.truncate(k);
        Ok(page)
    }

    fn thread(&self, q: &ThreadQuery) -> Result<(Thread, ThreadMeta), AppError> {
        if !matches!(q.sort, ThreadSort::Top | ThreadSort::New | ThreadSort::Old) {
            return Err(SOURCE.unsupported(format_args!("--sort {}", q.sort.as_str())));
        }
        let rows = self.records("/posts/ids", &[("ids", q.post.as_str())])?;
        let post = parse::post(exact(&rows, "id", q.post.as_str(), "post")?)?;
        let parent_id = q.comment.as_ref().map(|id| format!("t1_{id}"));
        let mut params = vec![("link_id", post.fullname.as_str()), ("limit", TREE_LIMIT)];
        if let Some(parent_id) = &parent_id {
            params.push(("parent_id", parent_id));
        }
        let rows = self.records("/comments/tree", &params)?;
        let parsed = parse::comment_tree(&rows)?;
        if let Some(parent_id) = &parent_id
            && !parsed
                .comments
                .iter()
                .any(|comment| comment.fullname == *parent_id)
        {
            return Err(AppError::new(
                ErrorCode::ENotFound,
                "archive comment not found",
            ));
        }
        let comments = tree::rebuild(&post.fullname, parsed.comments);
        let mut notes = Vec::new();
        if !parsed.unresolved.is_empty() {
            notes.push(format!(
                "Archive tree contains collapsed replies ({} hidden comments counted).",
                parsed.more_count
            ));
        }
        let mut thread = Thread {
            post,
            comments_returned: tree::count(&comments),
            comments,
            complete: parsed.unresolved.is_empty(),
            unresolved: parsed.unresolved,
        };
        tree::prune_thread(&mut thread, Some(q.sort), q.depth, q.limit, &mut notes);
        Ok((
            thread,
            ThreadMeta {
                more_count: Some(parsed.more_count),
                requests: Some(2),
                notes,
            },
        ))
    }

    fn user_about(&self, name: &UserName) -> Result<User, AppError> {
        let rows = self.records("/users/search", &[("author", name.as_str())])?;
        parse::user(exact(&rows, "author", name.as_str(), "user")?)
    }

    fn user_items(&self, q: &UserQuery) -> Result<Page<Item>, AppError> {
        if q.sort != UserSort::New {
            return Err(SOURCE.unsupported(format_args!("--sort {}", q.sort.as_str())));
        }
        if q.time != Period::All {
            return Err(SOURCE.unsupported("--time (use --since/--until)"));
        }
        let bounds = Bounds::new(q.since, q.until, q.after.as_deref())?;
        let listing = |endpoint| {
            self.timeline(
                endpoint,
                vec![("author", q.name.as_str())],
                q.limit,
                bounds,
                None,
            )
        };
        let posts = || {
            map_page(listing("/posts/search")?, |row| {
                parse::post(row).map(Item::Post)
            })
        };
        let comments = || {
            map_page(listing("/comments/search")?, |row| {
                parse::comment(row).map(Item::Comment)
            })
        };
        let mut page = match q.listing {
            UserListing::Posts => posts()?,
            UserListing::Comments => comments()?,
            UserListing::Overview => merge_overview(posts()?, comments()?, q.limit),
        };
        page.notes.push(USER_DATE_NOTE.into());
        Ok(page)
    }
}

fn cursor(created: i64, id: &str) -> String {
    format!("a:{created}:{id}")
}

fn single_sub(name: &SubName) -> Result<(), AppError> {
    if name.is_multi() {
        return Err(SOURCE.unsupported("multireddits"));
    }
    Ok(())
}

fn reject_name_cursor(q: &SearchQuery) -> Result<(), AppError> {
    match q.after {
        Some(_) => Err(SOURCE.unsupported("--after for subreddit and user search")),
        None => Ok(()),
    }
}

fn validate_search(q: &SearchQuery) -> Result<(), AppError> {
    if !matches!(q.sort, SearchSort::New | SearchSort::Relevance) {
        return Err(SOURCE.unsupported(format_args!("--sort {}", q.sort.as_str())));
    }
    if q.time != Period::All {
        return Err(SOURCE.unsupported("--time (use --since/--until)"));
    }
    Ok(())
}

fn exact<'a>(
    rows: &'a [Value],
    field: &str,
    name: &str,
    kind: &str,
) -> Result<&'a Value, AppError> {
    rows.iter()
        .find(|row| {
            row[field]
                .as_str()
                .is_some_and(|value| value.eq_ignore_ascii_case(name))
        })
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::ENotFound,
                format!("archive {kind} not found: {name}"),
            )
        })
}

fn map_page<T>(
    raw: Page<Value>,
    parse: impl Fn(&Value) -> Result<T, AppError>,
) -> Result<Page<T>, AppError> {
    Ok(Page {
        items: raw.items.iter().map(parse).collect::<Result<_, _>>()?,
        after: raw.after,
        notes: raw.notes,
    })
}

fn item_key(item: &Item) -> (i64, &str) {
    match item {
        Item::Post(post) => (post.created_utc, &post.id),
        Item::Comment(comment) => (comment.created_utc, &comment.id),
    }
}

fn merge_overview(mut posts: Page<Item>, comments: Page<Item>, limit: usize) -> Page<Item> {
    let has_more = posts.after.is_some()
        || comments.after.is_some()
        || posts.items.len() + comments.items.len() > limit;
    posts.items.extend(comments.items);
    posts
        .items
        .sort_by(|left, right| item_key(right).cmp(&item_key(left)));
    posts.items.truncate(limit);
    posts.after = if has_more {
        posts.items.last().map(|item| {
            let (created, id) = item_key(item);
            cursor(created, id)
        })
    } else {
        None
    };
    posts.notes.extend(comments.notes);
    posts
}
