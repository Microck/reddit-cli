use super::{Output, date_range};
use crate::{
    backend::{Backend, Page, PostId, SearchQuery, SourceKind},
    cli::{Period, Rank, SearchArgs, SearchSort, SearchType},
    error::AppError,
    ids::{SubName, UserName},
    model::Post,
};
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    thread,
};

pub fn run(backend: &dyn Backend, args: SearchArgs) -> Result<Output, AppError> {
    let source = backend.kind();
    if args.query.trim().is_empty() {
        return Err(AppError::usage("search query must not be empty"));
    }
    let subs = args
        .sub
        .iter()
        .map(|raw| SubName::parse(raw))
        .collect::<Result<Vec<_>, _>>()?;
    let author = args.author.as_deref().map(UserName::parse).transpose()?;
    let multi = subs.len() > 1;
    if multi && args.after.is_some() {
        return Err(AppError::usage("--after requires at most one --sub"));
    }
    if args.search_type != SearchType::Posts && (multi || args.comments > 0) {
        return Err(AppError::usage(
            "multiple --sub values and --comments require --type posts",
        ));
    }
    if matches!(args.search_type, SearchType::Subs | SearchType::Users)
        && (!subs.is_empty() || author.is_some() || args.since.is_some() || args.until.is_some())
    {
        return Err(AppError::usage(
            "subreddit and user searches accept a name query without item filters",
        ));
    }

    validate_source(source, &args, &subs, author.is_some())?;
    let (since, until) = date_range(args.since.as_deref(), args.until.as_deref())?;
    let query = SearchQuery {
        query: args.query,
        sub: subs.first().cloned(),
        sort: if source == SourceKind::Archive {
            SearchSort::New
        } else {
            args.sort
        },
        time: args.time.unwrap_or(Period::All),
        author,
        since,
        until,
        limit: args.limit,
        after: source.cursor(args.after)?,
    };
    match args.search_type {
        SearchType::Posts => {
            let mut page = if multi {
                multi_sub(backend, &query, &subs, args.rank)?
            } else {
                backend.search_posts(&query)?
            };
            if args.comments > 0 {
                attach_comments(backend, &mut page, usize::from(args.comments));
            }
            Ok(Output::page(page, source))
        }
        SearchType::Comments => {
            let mut page = backend.search_comments(&query)?;
            for comment in &mut page.items {
                comment.replies.clear();
            }
            Ok(Output::page(page, source))
        }
        SearchType::Subs => Ok(Output::page(backend.search_subreddits(&query)?, source)),
        SearchType::Users => Ok(Output::page(backend.search_users(&query)?, source)),
    }
}

fn validate_source(
    source: SourceKind,
    args: &SearchArgs,
    subs: &[SubName],
    has_author: bool,
) -> Result<(), AppError> {
    match source {
        SourceKind::Live => {
            if args.search_type == SearchType::Comments {
                return Err(source.unsupported("--type comments"));
            }
            if has_author || args.since.is_some() || args.until.is_some() {
                return Err(source.unsupported("--author/--since/--until"));
            }
        }
        SourceKind::Archive => {
            if args.after.is_some()
                && matches!(args.search_type, SearchType::Subs | SearchType::Users)
            {
                return Err(source.unsupported("--after for subreddit and user search"));
            }
            if subs.iter().any(SubName::is_multi) {
                return Err(source.unsupported("multireddits"));
            }
            if args.time.is_some() {
                return Err(source.unsupported("--time (use --since/--until)"));
            }
            if !matches!(args.sort, SearchSort::New | SearchSort::Relevance) {
                return Err(source.unsupported(format_args!("--sort {}", args.sort.as_str())));
            }
        }
    }
    Ok(())
}

fn multi_sub(
    backend: &dyn Backend,
    query: &SearchQuery,
    subs: &[SubName],
    rank: Rank,
) -> Result<Page<Post>, AppError> {
    let pages = parallel(subs, |sub| {
        let mut query = query.clone();
        query.sub = Some(sub.clone());
        backend.search_posts(&query)
    });
    let mut rows = Vec::new();
    let mut notes = Vec::new();
    let mut first_error = None;
    for (sub, page) in subs.iter().zip(pages) {
        match page {
            Ok(page) => {
                rows.push(page.items);
                notes.extend(page.notes);
            }
            Err(error) => {
                notes.push(format!("r/{sub}: {error}"));
                first_error.get_or_insert(error);
            }
        }
    }
    if rows.is_empty()
        && let Some(error) = first_error
    {
        return Err(error);
    }
    Ok(Page {
        items: ranked(rows, rank, query.limit),
        after: None,
        notes,
    })
}

fn attach_comments(backend: &dyn Backend, page: &mut Page<Post>, count: usize) {
    let replies = parallel(&page.items, |post| {
        let id = PostId::parse(&post.id)?;
        backend.top_comments(&id, count)
    });
    for (post, comments) in page.items.iter_mut().zip(replies) {
        let mut comments = match comments {
            Ok(comments) => {
                page.notes.extend(comments.notes);
                comments.items
            }
            Err(error) => {
                page.notes.push(format!(
                    "Post {}: could not fetch top comments ({error}).",
                    post.id
                ));
                Vec::new()
            }
        };
        comments.truncate(count);
        for comment in &mut comments {
            comment.replies.clear();
        }
        post.top_comments = Some(comments);
    }
}

/// Return results in input order, even when individual requests finish out of order.
fn parallel<T: Sync, U: Send>(items: &[T], work: impl Fn(&T) -> U + Sync) -> Vec<U> {
    let mut results = Vec::with_capacity(items.len());
    for batch in items.chunks(4) {
        thread::scope(|scope| {
            let jobs: Vec<_> = batch
                .iter()
                .map(|item| scope.spawn(|| work(item)))
                .collect();
            results.extend(
                jobs.into_iter()
                    .map(|job| job.join().expect("backend worker panicked")),
            );
        });
    }
    results
}

fn ranked(pages: Vec<Vec<Post>>, rank: Rank, limit: usize) -> Vec<Post> {
    let mut pages: Vec<_> = pages.into_iter().map(Vec::into_iter).collect();
    let mut interleaved = Vec::new();
    let mut seen = HashSet::new();
    while !pages.is_empty() {
        pages.retain_mut(|page| {
            if let Some(post) = page.next() {
                if seen.insert(post.id.clone()) {
                    interleaved.push(post);
                }
                true
            } else {
                false
            }
        });
    }

    // The original post participates in the same group as its crossposts.
    let family = |post: &Post| {
        post.crosspost_parent
            .as_deref()
            .unwrap_or(&post.id)
            .trim_start_matches("t3_")
            .to_owned()
    };
    let mut winners = HashMap::new();
    for (index, post) in interleaved.iter().enumerate() {
        let previous: &mut usize = winners.entry(family(post)).or_insert(index);
        if post.score > interleaved[*previous].score {
            *previous = index;
        }
    }
    let mut posts: Vec<_> = interleaved
        .into_iter()
        .enumerate()
        .filter_map(|(index, post)| (winners[&family(&post)] == index).then_some(post))
        .collect();
    match rank {
        Rank::Interleave => {}
        Rank::Score => posts.sort_by_key(|post| Reverse(post.score)),
        Rank::New => posts.sort_by_key(|post| Reverse(post.created_utc)),
        Rank::Comments => posts.sort_by_key(|post| Reverse(post.num_comments)),
    }
    posts.truncate(limit);
    posts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(id: &str, score: Option<i64>, parent: Option<&str>) -> Post {
        Post {
            id: id.into(),
            score,
            crosspost_parent: parent.map(str::to_owned),
            ..Post::default()
        }
    }

    #[test]
    fn interleaves_then_dedupes_and_prefers_highest_crosspost_score() {
        let pages = vec![
            vec![
                post("aa", Some(1), None),
                post("bb", None, None),
                post("cc", Some(2), None),
            ],
            vec![
                post("zz", Some(3), Some("t3_aa")),
                post("cc", Some(2), None),
            ],
        ];
        let ids = |rank| {
            ranked(pages.clone(), rank, 3)
                .into_iter()
                .map(|post| post.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(Rank::Interleave), ["zz", "bb", "cc"]);
        assert_eq!(ids(Rank::Score), ["zz", "cc", "bb"]);
    }
}
