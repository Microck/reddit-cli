use crate::{
    backend::CommentId,
    error::{AppError, ErrorCode},
    model::{
        Comment, Item, Media, MediaType, Post, State, Subreddit, User, UserSummary, timestamp,
    },
};
use reqwest::Url;
use serde_json::Value;

const REDDIT: &str = "https://www.reddit.com";

pub(super) enum More {
    Children(Vec<CommentId>),
    Continue(CommentId),
}

#[derive(Default)]
pub(super) struct Comments {
    pub comments: Vec<Comment>,
    pub more: Vec<More>,
}

pub(super) fn listing(value: &Value) -> Result<(&[Value], Option<&str>), AppError> {
    let data = thing_data(value, "Listing")?;
    let children = array(&data["children"], "listing children")?;
    let after = match &data["after"] {
        Value::Null => None,
        Value::String(after) => Some(after.as_str()),
        _ => return Err(malformed("listing cursor")),
    };
    Ok((children, after))
}

pub(super) fn post(value: &Value) -> Result<Post, AppError> {
    let data = thing_data(value, "t3")?;
    let id = required_text(data, "id")?;
    let author = text(data, "author");
    let body = text(data, "selftext");
    let state = state(data, author, body);
    let created_utc = created_utc(data);
    let is_self = flag(data, "is_self");
    let url = nonempty(&data["url"]).or_else(|| nonempty(&data["url_overridden_by_dest"]));
    Ok(Post {
        id: id.into(),
        fullname: format!("t3_{id}"),
        subreddit: text(data, "subreddit").into(),
        author: author.into(),
        title: text(data, "title").into(),
        selftext: body.into(),
        url: url.unwrap_or_default().into(),
        domain: text(data, "domain").into(),
        permalink: reddit_url(text(data, "permalink"), || format!("/comments/{id}/")),
        created_utc,
        created: timestamp(created_utc),
        score: data["score"].as_i64(),
        upvote_ratio: data["upvote_ratio"].as_f64(),
        num_comments: data["num_comments"].as_i64(),
        over_18: flag(data, "over_18"),
        spoiler: flag(data, "spoiler"),
        stickied: flag(data, "stickied"),
        locked: flag(data, "locked"),
        is_self,
        flair: nonempty(&data["link_flair_text"]).map(str::to_owned),
        crosspost_parent: nonempty(&data["crosspost_parent"]).map(str::to_owned),
        media: media(data, url, is_self),
        state,
        top_comments: None,
    })
}

fn comment(data: &Value) -> Result<Comment, AppError> {
    let id = required_text(data, "id")?;
    let link_id = required_text(data, "link_id")?;
    let author = text(data, "author");
    let body = text(data, "body");
    let state = state(data, author, body);
    let created_utc = created_utc(data);
    Ok(Comment {
        id: id.into(),
        fullname: format!("t1_{id}"),
        link_id: link_id.into(),
        parent_id: required_text(data, "parent_id")?.into(),
        subreddit: text(data, "subreddit").into(),
        author: author.into(),
        body: body.into(),
        created_utc,
        created: timestamp(created_utc),
        score: data["score"].as_i64(),
        depth: data["depth"].as_u64().unwrap_or_default() as usize,
        is_submitter: flag(data, "is_submitter"),
        stickied: flag(data, "stickied"),
        distinguished: nonempty(&data["distinguished"]).map(str::to_owned),
        edited: data["edited"]
            .as_bool()
            .unwrap_or_else(|| data["edited"].is_number()),
        state,
        permalink: reddit_url(text(data, "permalink"), || {
            let post = link_id.trim_start_matches("t3_");
            format!("/comments/{post}/_/{id}/")
        }),
        replies: Vec::new(),
    })
}

pub(super) fn subreddit(value: &Value) -> Result<Subreddit, AppError> {
    let data = thing_data(value, "t5")?;
    let name = required_text(data, "display_name")?;
    let created_utc = created_utc(data);
    Ok(Subreddit {
        name: name.into(),
        title: text(data, "title").into(),
        description: text(data, "public_description").into(),
        subscribers: data["subscribers"].as_i64(),
        active_users: data["accounts_active"]
            .as_i64()
            .or_else(|| data["active_user_count"].as_i64()),
        created_utc,
        created: timestamp(created_utc),
        over_18: flag(data, "over18"),
        url: reddit_url(text(data, "url"), || format!("/r/{name}/")),
    })
}

pub(super) fn user(value: &Value) -> Result<User, AppError> {
    let data = thing_data(value, "t2")?;
    let created_utc = created_utc(data);
    let link_karma = data["link_karma"].as_i64().unwrap_or_default();
    let comment_karma = data["comment_karma"].as_i64().unwrap_or_default();
    Ok(User {
        name: required_text(data, "name")?.into(),
        created_utc,
        created: timestamp(created_utc),
        link_karma,
        comment_karma,
        total_karma: data["total_karma"]
            .as_i64()
            .unwrap_or_else(|| link_karma.saturating_add(comment_karma)),
        is_suspended: flag(data, "is_suspended"),
        is_employee: flag(data, "is_employee"),
    })
}

pub(super) fn user_summary(value: &Value) -> Result<UserSummary, AppError> {
    let data = thing_data(value, "t2")?;
    let created_utc = created_utc(data);
    Ok(UserSummary {
        name: required_text(data, "name")?.into(),
        created_utc,
        created: timestamp(created_utc),
        total_karma: data["total_karma"].as_i64(),
    })
}

pub(super) fn item(value: &Value) -> Result<Item, AppError> {
    match value["kind"].as_str() {
        Some("t3") => post(value).map(Item::Post),
        Some("t1") => comment(thing_data(value, "t1")?).map(Item::Comment),
        _ => Err(malformed("user listing item")),
    }
}

pub(super) fn thread(value: &Value) -> Result<(Post, Comments), AppError> {
    let (posts, _) = listing(&value[0])?;
    let post = posts
        .first()
        .ok_or_else(|| AppError::new(ErrorCode::ENotFound, "Reddit post not found"))?;
    let (things, _) = listing(&value[1])?;
    Ok((self::post(post)?, parse_comments(things)?))
}

pub(super) fn morechildren(value: &Value) -> Result<Comments, AppError> {
    if !array(&value["json"]["errors"], "morechildren errors")?.is_empty() {
        return Err(AppError::upstream("Reddit rejected a morechildren request"));
    }
    let things = array(&value["json"]["data"]["things"], "morechildren things")?;
    parse_comments(things)
}

fn parse_comments(things: &[Value]) -> Result<Comments, AppError> {
    let mut parsed = Comments::default();
    let mut stack = vec![things.iter()];
    while let Some(things) = stack.last_mut() {
        let Some(thing) = things.next() else {
            stack.pop();
            continue;
        };
        match thing["kind"].as_str() {
            Some("t1") => {
                let data = thing_data(thing, "t1")?;
                parsed.comments.push(comment(data)?);
                let replies = &data["replies"];
                if !replies.is_null() && replies.as_str() != Some("") {
                    let (children, _) = listing(replies)?;
                    stack.push(children.iter());
                }
            }
            Some("more") => parsed.more.push(more(thing_data(thing, "more")?)?),
            _ => return Err(malformed("thread comment or more stub")),
        }
    }
    Ok(parsed)
}

fn more(data: &Value) -> Result<More, AppError> {
    let children = array(&data["children"], "more children")?;
    if children.is_empty() {
        let parent = required_text(data, "parent_id")?
            .strip_prefix("t1_")
            .ok_or_else(|| malformed("continuation parent"))?;
        return Ok(More::Continue(parse_comment_id(parent)?));
    }
    let ids = children
        .iter()
        .map(|value| parse_comment_id(value.as_str().unwrap_or_default()))
        .collect::<Result<_, _>>()?;
    Ok(More::Children(ids))
}

fn parse_comment_id(id: &str) -> Result<CommentId, AppError> {
    CommentId::parse(id).map_err(|_| malformed("more comment ID"))
}

fn malformed(field: &str) -> AppError {
    AppError::upstream(format!("invalid Reddit response: {field}"))
}

fn array<'a>(value: &'a Value, field: &str) -> Result<&'a [Value], AppError> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| malformed(field))
}

fn thing_data<'a>(value: &'a Value, kind: &str) -> Result<&'a Value, AppError> {
    if value["kind"].as_str() != Some(kind) || !value["data"].is_object() {
        return Err(malformed(kind));
    }
    Ok(&value["data"])
}

fn required_text<'a>(data: &'a Value, key: &str) -> Result<&'a str, AppError> {
    nonempty(&data[key]).ok_or_else(|| malformed(key))
}

fn nonempty(value: &Value) -> Option<&str> {
    value.as_str().filter(|text| !text.is_empty())
}

fn text<'a>(data: &'a Value, key: &str) -> &'a str {
    data[key].as_str().unwrap_or_default()
}

fn flag(data: &Value, key: &str) -> bool {
    data[key].as_bool().unwrap_or_default()
}

fn created_utc(data: &Value) -> i64 {
    // Reddit serializes these seconds as either integers or JSON floats.
    data["created_utc"]
        .as_f64()
        .map_or(0, |seconds| seconds as i64)
}

fn state(data: &Value, author: &str, body: &str) -> State {
    if author == "[deleted]" && body == "[deleted]" {
        State::Deleted
    } else if body == "[removed]" || nonempty(&data["removed_by_category"]).is_some() {
        State::Removed
    } else {
        State::Ok
    }
}

fn reddit_url(path: &str, fallback: impl FnOnce() -> String) -> String {
    if path.starts_with('/') && !path.starts_with("//") {
        return format!("{REDDIT}{path}");
    }
    if let Ok(url) = Url::parse(path)
        && url
            .host_str()
            .is_some_and(|host| host == "reddit.com" || host.ends_with(".reddit.com"))
    {
        return format!("{REDDIT}{}", url.path());
    }
    format!("{REDDIT}{}", fallback())
}

fn media(data: &Value, url: Option<&str>, is_self: bool) -> Media {
    if flag(data, "is_gallery") || data["gallery_data"]["items"].is_array() {
        let urls = data["gallery_data"]["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| {
                let id = nonempty(&item["media_id"])?;
                let rendition = &data["media_metadata"][id]["s"];
                ["u", "gif", "mp4"]
                    .iter()
                    .find_map(|key| nonempty(&rendition[*key]))
                    .map(decode_url)
            })
            .collect();
        return Media {
            kind: MediaType::Gallery,
            urls,
        };
    }

    let destination = nonempty(&data["url_overridden_by_dest"]).or(url.filter(|_| !is_self));
    let preview = nonempty(&data["preview"]["images"][0]["source"]["url"]);
    let fallback = nonempty(&data["secure_media"]["reddit_video"]["fallback_url"]);
    let (kind, url) = if flag(data, "is_video") || fallback.is_some() {
        (MediaType::Video, fallback.or(destination).or(preview))
    } else if text(data, "post_hint") == "image" || destination.is_some_and(image_url) {
        (MediaType::Image, destination.or(preview))
    } else if destination.is_some() {
        (MediaType::Link, destination)
    } else if preview.is_some() {
        (MediaType::Image, preview)
    } else {
        return Media::default();
    };
    Media {
        kind,
        urls: url.map(decode_url).into_iter().collect(),
    }
}

fn image_url(url: &str) -> bool {
    url.split(['?', '#'])
        .next()
        .and_then(|path| path.rsplit_once('.'))
        .is_some_and(|(_, extension)| {
            ["png", "jpg", "jpeg", "gif", "webp", "avif"]
                .iter()
                .any(|image| extension.eq_ignore_ascii_case(image))
        })
}

fn decode_url(url: &str) -> String {
    // Decode one layer. Query values can contain a literal entity spelling.
    let mut parts = url.split('&');
    let mut decoded = String::with_capacity(url.len());
    decoded.push_str(parts.next().unwrap_or_default());
    for part in parts {
        if let Some((entity, rest)) = part.split_once(';')
            && let Some(character) = decode_entity(entity)
        {
            decoded.push(character);
            decoded.push_str(rest);
        } else {
            decoded.push('&');
            decoded.push_str(part);
        }
    }
    decoded
}

fn decode_entity(entity: &str) -> Option<char> {
    Some(match entity {
        "amp" => '&',
        "quot" => '"',
        "apos" => '\'',
        "lt" => '<',
        "gt" => '>',
        _ => {
            let numeric = entity.strip_prefix('#')?;
            let code = match numeric.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16),
                None => numeric.parse(),
            };
            char::from_u32(code.ok()?)?
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn post_removal_category_sets_state_and_normalizes_time_and_permalink() {
        let value = json!({
            "kind": "t3", "data": {
                "id": "abc", "author": "user", "selftext": "previous body", "title": "previous title",
                "removed_by_category": "moderator", "created_utc": 1700000000.0,
                "permalink": "http://old.reddit.com/r/rust/comments/abc/"
            }
        });
        let post = post(&value).unwrap();
        assert_eq!(post.state, State::Removed);
        assert_eq!(post.title, "previous title");
        assert_eq!(post.selftext, "previous body");
        assert_eq!(post.created, "2023-11-14T22:13:20Z");
        assert_eq!(
            post.permalink,
            "https://www.reddit.com/r/rust/comments/abc/"
        );
    }

    #[test]
    fn deleted_comments_distinguish_author_deletion_from_marker_text() {
        let mut data = json!({
            "id": "def", "link_id": "t3_abc", "parent_id": "t3_abc",
            "body": "[deleted]", "author": "[deleted]", "edited": 1700000000.0
        });
        let deleted = comment(&data).unwrap();
        assert_eq!(deleted.state, State::Deleted);
        assert!(deleted.edited);
        data["author"] = json!("someone");
        assert_eq!(comment(&data).unwrap().state, State::Ok);
    }

    #[test]
    fn media_uses_gallery_order_video_rendition_and_decoded_preview() {
        let gallery = json!({
            "gallery_data": {"items": [{"media_id": "b"}, {"media_id": "a"}]},
            "media_metadata": {
                "a": {"s": {"u": "https://i.redd.it/a.png"}},
                "b": {"s": {"gif": "https://i.redd.it/b.gif?x=1&amp;y=2"}}
            }
        });
        let gallery = media(&gallery, None, false);
        assert!(matches!(gallery.kind, MediaType::Gallery));
        assert_eq!(
            gallery.urls,
            ["https://i.redd.it/b.gif?x=1&y=2", "https://i.redd.it/a.png"]
        );

        let video = json!({
            "url_overridden_by_dest": "https://v.redd.it/video",
            "secure_media": {"reddit_video": {"fallback_url": "https://v.redd.it/video/DASH.mp4"}}
        });
        let video = media(&video, None, false);
        assert!(matches!(video.kind, MediaType::Video));
        assert_eq!(video.urls, ["https://v.redd.it/video/DASH.mp4"]);

        let preview = json!({
            "post_hint": "image",
            "preview": {"images": [{"source": {"url": "https://preview.redd.it/a?x=1&#x26;y=2"}}]}
        });
        let preview = media(&preview, None, false);
        assert!(matches!(preview.kind, MediaType::Image));
        assert_eq!(preview.urls, ["https://preview.redd.it/a?x=1&y=2"]);
    }

    #[test]
    fn more_stubs_reject_invalid_comment_ids() {
        assert!(more(&json!({"children": ["abc?x=1"]})).is_err());
        assert!(more(&json!({"children": [], "parent_id": "t3_abc"})).is_err());
        let stub = more(&json!({"children": [], "parent_id": "t1_abc"})).unwrap();
        assert!(matches!(stub, More::Continue(id) if id.as_str() == "abc"));
    }
}
