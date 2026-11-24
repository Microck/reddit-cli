use crate::{
    error::AppError,
    model::{
        Comment, Media, MediaType, Post, State, Subreddit, Unresolved, User, UserSummary, timestamp,
    },
};
use reqwest::Url;
use serde_json::Value;
use std::collections::HashSet;

const REDDIT: &str = "https://www.reddit.com";

fn text<'a>(row: &'a Value, field: &str) -> &'a str {
    row[field].as_str().unwrap_or_default()
}

fn flag(row: &Value, field: &str) -> bool {
    row[field].as_bool().unwrap_or_default()
}

fn optional_text(row: &Value, field: &str) -> Option<String> {
    row[field].as_str().map(str::to_owned)
}

fn absolute(path: &str, fallback: impl FnOnce() -> String) -> String {
    if path.is_empty() {
        return format!("{REDDIT}{}", fallback());
    }
    let url = Url::parse(path).ok();
    let path = url.as_ref().map_or(path, Url::path);
    format!("{REDDIT}/{}", path.trim_start_matches('/'))
}

fn state(row: &Value, author: &str, body: &str) -> State {
    if author == "[deleted]" && body == "[deleted]" {
        State::Deleted
    } else if body == "[removed]" || !row["removed_by_category"].is_null() {
        State::Removed
    } else if flag(&row["_meta"], "was_deleted_later") {
        // The archive may keep the original text after observing a removal.
        match text(&row["_meta"], "removal_type") {
            "author" | "deleted" => State::Deleted,
            "moderator" | "reddit" => State::Removed,
            _ => State::Ok,
        }
    } else {
        State::Ok
    }
}

fn refreshed(row: &Value) -> bool {
    !row["_meta"]["retrieved_2nd_on"].is_null()
}

pub(super) fn key(row: &Value) -> Result<(&str, i64), AppError> {
    match (row["id"].as_str(), row["created_utc"].as_i64()) {
        (Some(id), Some(created)) if !id.is_empty() => Ok((id, created)),
        _ => Err(AppError::upstream(
            "archive record is missing its ID or timestamp",
        )),
    }
}

pub(super) fn post(row: &Value) -> Result<Post, AppError> {
    let (id, created_utc) = key(row)?;
    let author = text(row, "author");
    let selftext = text(row, "selftext");
    let state = state(row, author, selftext);
    let refreshed = refreshed(row);
    let url = text(row, "url");
    let is_self = flag(row, "is_self");
    Ok(Post {
        id: id.into(),
        fullname: format!("t3_{id}"),
        subreddit: text(row, "subreddit").into(),
        author: author.into(),
        title: text(row, "title").into(),
        selftext: state.marker().unwrap_or(selftext).into(),
        url: url.into(),
        domain: text(row, "domain").into(),
        permalink: absolute(text(row, "permalink"), || format!("/comments/{id}/")),
        created_utc,
        created: timestamp(created_utc),
        score: row["score"].as_i64().filter(|_| refreshed),
        upvote_ratio: row["upvote_ratio"].as_f64().filter(|_| refreshed),
        num_comments: row["num_comments"].as_i64().filter(|_| refreshed),
        over_18: flag(row, "over_18"),
        spoiler: flag(row, "spoiler"),
        stickied: flag(row, "stickied"),
        locked: flag(row, "locked"),
        is_self,
        flair: optional_text(row, "link_flair_text"),
        crosspost_parent: optional_text(row, "crosspost_parent"),
        media: media(row, url, is_self),
        state,
        top_comments: None,
    })
}

pub(super) fn comment(row: &Value) -> Result<Comment, AppError> {
    let (id, created_utc) = key(row)?;
    let author = text(row, "author");
    let body = text(row, "body");
    let state = state(row, author, body);
    let link_id = text(row, "link_id");
    let parent_id = text(row, "parent_id");
    Ok(Comment {
        id: id.into(),
        fullname: format!("t1_{id}"),
        link_id: link_id.into(),
        parent_id: parent_id.into(),
        subreddit: text(row, "subreddit").into(),
        author: author.into(),
        body: state.marker().unwrap_or(body).into(),
        created_utc,
        created: timestamp(created_utc),
        score: row["score"].as_i64().filter(|_| refreshed(row)),
        depth: row["depth"].as_u64().unwrap_or_default() as usize,
        is_submitter: flag(row, "is_submitter"),
        stickied: flag(row, "stickied"),
        distinguished: optional_text(row, "distinguished"),
        edited: flag(row, "edited") || row["edited"].as_f64().is_some_and(|value| value > 0.0),
        state,
        permalink: absolute(text(row, "permalink"), || {
            let post_id = link_id.trim_start_matches("t3_");
            format!("/comments/{post_id}/_/{id}/")
        }),
        replies: Vec::new(),
    })
}

pub(super) fn subreddit(row: &Value) -> Result<Subreddit, AppError> {
    let name = text(row, "display_name");
    if name.is_empty() {
        return Err(AppError::upstream(
            "archive subreddit record is missing its name",
        ));
    }
    let created_utc = row["created_utc"].as_i64().unwrap_or_default();
    let description = row["public_description"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| text(row, "description"));
    Ok(Subreddit {
        name: name.into(),
        title: text(row, "title").into(),
        description: description.into(),
        subscribers: row["subscribers"].as_i64(),
        active_users: row["active_user_count"].as_i64(),
        created_utc,
        created: timestamp(created_utc),
        over_18: flag(row, "over18"),
        url: absolute(text(row, "url"), || format!("/r/{name}/")),
    })
}

// The users endpoint aggregates activity and usually has no account creation time.
pub(super) fn user_summary(row: &Value) -> Result<UserSummary, AppError> {
    let name = text(row, "author");
    if name.is_empty() {
        return Err(AppError::upstream(
            "archive user record is missing its name",
        ));
    }
    let meta = &row["_meta"];
    let created_utc = row["created_utc"]
        .as_i64()
        .or_else(|| {
            ["earliest_post_at", "earliest_comment_at"]
                .into_iter()
                .filter_map(|field| meta[field].as_i64())
                .min()
        })
        .unwrap_or_default();
    Ok(UserSummary {
        name: name.into(),
        created_utc,
        created: timestamp(created_utc),
        total_karma: meta["total_karma"].as_i64(),
    })
}

pub(super) fn user(row: &Value) -> Result<User, AppError> {
    let summary = user_summary(row)?;
    let meta = &row["_meta"];
    let link_karma = meta["post_karma"].as_i64().unwrap_or_default();
    let comment_karma = meta["comment_karma"].as_i64().unwrap_or_default();
    Ok(User {
        name: summary.name,
        created_utc: summary.created_utc,
        created: summary.created,
        link_karma,
        comment_karma,
        total_karma: summary
            .total_karma
            .unwrap_or_else(|| link_karma.saturating_add(comment_karma)),
        is_suspended: flag(row, "is_suspended"),
        is_employee: flag(row, "is_employee"),
    })
}

pub(super) struct CommentTree {
    pub comments: Vec<Comment>,
    pub unresolved: Vec<Unresolved>,
    pub more_count: usize,
}

pub(super) fn comment_tree(rows: &[Value]) -> Result<CommentTree, AppError> {
    let mut comments = Vec::new();
    let mut unresolved = Vec::new();
    let mut hidden = HashSet::new();
    let mut more_count = 0;
    let mut pending: Vec<_> = rows.iter().rev().collect();
    while let Some(thing) = pending.pop() {
        let row = &thing["data"];
        match thing["kind"].as_str() {
            Some("t1") => {
                comments.push(comment(row)?);
                if let Some(children) = row["replies"]["data"]["children"].as_array() {
                    pending.extend(children.iter().rev());
                }
            }
            Some("more") => {
                let children = row["children"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                if children.is_empty() {
                    // A stub can indicate hidden replies without naming the IDs.
                    let id = row["parent_id"].as_str().unwrap_or_else(|| text(row, "id"));
                    more_count += row["count"].as_u64().unwrap_or_default() as usize;
                    unresolved.push(Unresolved {
                        id: id.into(),
                        reason: "archive tree collapsed replies without comment IDs".into(),
                    });
                }
                for id in children.iter().filter_map(Value::as_str) {
                    let id = id.trim_start_matches("t1_");
                    if hidden.insert(id) {
                        more_count += 1;
                        unresolved.push(Unresolved {
                            id: id.into(),
                            reason: "archive tree collapsed this comment".into(),
                        });
                    }
                }
            }
            _ => {
                return Err(AppError::upstream(
                    "archive tree returned an unknown item kind",
                ));
            }
        }
    }
    Ok(CommentTree {
        comments,
        unresolved,
        more_count,
    })
}

fn media(row: &Value, url: &str, is_self: bool) -> Media {
    let destination = row["url_overridden_by_dest"].as_str().unwrap_or(url);
    let preview = row["preview"]["images"][0]["source"]["url"].as_str();
    let video = row["secure_media"]["reddit_video"]["fallback_url"].as_str();
    let (kind, urls) = if let Some(items) = row["gallery_data"]["items"].as_array() {
        let urls = items
            .iter()
            .filter_map(|item| item["media_id"].as_str())
            .filter_map(|id| {
                let source = &row["media_metadata"][id]["s"];
                ["u", "gif", "mp4"]
                    .into_iter()
                    .find_map(|field| source[field].as_str())
            })
            .collect();
        (MediaType::Gallery, urls)
    } else if video.is_some() || flag(row, "is_video") || text(row, "post_hint").ends_with(":video")
    {
        (MediaType::Video, vec![video.unwrap_or(destination)])
    } else if text(row, "post_hint") == "image"
        || text(row, "domain") == "i.redd.it"
        || is_image_url(destination)
    {
        let image = if destination.is_empty() {
            preview.unwrap_or_default()
        } else {
            destination
        };
        (MediaType::Image, vec![image])
    } else if is_self {
        (MediaType::None, Vec::new())
    } else if !destination.is_empty() {
        (MediaType::Link, vec![destination])
    } else if let Some(preview) = preview {
        (MediaType::Image, vec![preview])
    } else {
        (MediaType::None, Vec::new())
    };
    Media {
        kind,
        urls: urls
            .into_iter()
            .filter(|url| !url.is_empty())
            .map(decode_entities)
            .collect(),
    }
}

fn is_image_url(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or_default();
    path.rsplit_once('.').is_some_and(|(_, ext)| {
        matches!(
            ext.to_ascii_lowercase().as_str(),
            "jpg" | "jpeg" | "png" | "gif" | "webp" | "avif"
        )
    })
}

fn decode_entities(mut url: &str) -> String {
    let mut decoded = String::new();
    while let Some((prefix, tail)) = url.split_once('&') {
        decoded.push_str(prefix);
        if let Some((entity, rest)) = tail.split_once(';')
            && let Some(value) = match entity {
                "amp" => Some('&'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "lt" => Some('<'),
                "gt" => Some('>'),
                _ => entity.strip_prefix('#').and_then(|number| {
                    let value = if let Some(hex) = number.strip_prefix(['x', 'X']) {
                        u32::from_str_radix(hex, 16)
                    } else {
                        number.parse()
                    };
                    value.ok().and_then(char::from_u32)
                }),
            }
        {
            decoded.push(value);
            url = rest;
        } else {
            decoded.push('&');
            url = tail;
        }
    }
    decoded.push_str(url);
    decoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn metrics_require_a_second_retrieval_and_removals_use_markers() {
        let mut row = json!({
            "id": "ab", "created_utc": 100, "score": 5, "num_comments": 3, "upvote_ratio": 0.75,
            "author": "example", "selftext": "original", "body": "original",
            "permalink": "/r/rust/comments/ab/title"
        });
        let initial = post(&row).unwrap();
        assert_eq!(
            (initial.score, initial.num_comments, initial.upvote_ratio),
            (None, None, None)
        );
        assert_eq!(
            initial.permalink,
            "https://www.reddit.com/r/rust/comments/ab/title"
        );
        assert_eq!(comment(&row).unwrap().score, None);

        row["_meta"] = json!({"retrieved_2nd_on": 500, "was_deleted_later": true, "removal_type": "moderator"});
        let refreshed = post(&row).unwrap();
        assert_eq!(
            (
                refreshed.score,
                refreshed.num_comments,
                refreshed.upvote_ratio
            ),
            (Some(5), Some(3), Some(0.75))
        );
        assert_eq!(refreshed.state, State::Removed);
        assert_eq!(refreshed.selftext, "[removed]");
        row["author"] = json!("[deleted]");
        row["body"] = json!("[deleted]");
        let deleted = comment(&row).unwrap();
        assert_eq!(deleted.state, State::Deleted);
        assert_eq!(deleted.score, Some(5));
    }

    #[test]
    fn gallery_follows_item_order_and_decodes_media_urls() {
        let record = json!({
            "gallery_data": {"items": [{"media_id": "bb"}, {"media_id": "aa"}]},
            "media_metadata": {
                "aa": {"s": {"u": "https://i.redd.it/aa.png?x=1&#38;y=2"}},
                "bb": {"s": {"u": "https://i.redd.it/bb.png?x=1&amp;y=2"}}
            }
        });
        let gallery = media(&record, "", false);
        assert!(matches!(gallery.kind, MediaType::Gallery));
        assert_eq!(
            gallery.urls,
            [
                "https://i.redd.it/bb.png?x=1&y=2",
                "https://i.redd.it/aa.png?x=1&y=2"
            ]
        );
    }

    #[test]
    fn tree_collects_unique_hidden_ids_and_counts_unknown_replies() {
        let rows = json!([
            {"kind": "t1", "data": {"id": "aa", "created_utc": 1, "replies": {
                "kind": "Listing", "data": {"children": [
                    {"kind": "more", "data": {"children": ["bb", "t1_bb", "cc"]}}
                ]}
            }}},
            {"kind": "more", "data": {"parent_id": "t1_aa", "children": [], "count": 1}}
        ]);
        let parsed = comment_tree(rows.as_array().unwrap()).unwrap();
        assert_eq!(parsed.comments[0].id, "aa");
        assert_eq!(parsed.more_count, 3);
        assert_eq!(
            parsed
                .unresolved
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            ["bb", "cc", "t1_aa"]
        );
    }
}
