use serde_json::Value;
use std::fmt::Write;

pub fn render(data: &Value) -> String {
    let mut out = String::new();
    if let (Some(post), Some(comments)) = (data.get("post"), data["comments"].as_array()) {
        let _ = writeln!(out, "# {}", text(post, "title"));
        let _ = writeln!(
            out,
            "r/{} · u/{} · {} pts · {} comments · {} · {}\n",
            text(post, "subreddit"),
            text(post, "author"),
            number(post, "score"),
            number(post, "num_comments"),
            date(post),
            text(post, "permalink"),
        );
        // Self posts carry their text; link posts their target. A self post with no
        // text has url == permalink, which the header line already shows.
        let body = text(post, "selftext");
        let url = text(post, "url");
        let shown = if !body.is_empty() {
            body
        } else if url != text(post, "permalink") {
            url
        } else {
            ""
        };
        if shown.is_empty() {
            let _ = writeln!(out, "---\n");
        } else {
            let _ = writeln!(out, "{shown}\n\n---\n");
        }
        render_comments(&mut out, comments);
    } else if let Some(items) = data.as_array() {
        render_listing(&mut out, items);
    } else if let (Some(user), Some(items)) = (data.get("user"), data["items"].as_array()) {
        let _ = writeln!(out, "# u/{}\n", text(user, "name"));
        render_listing(&mut out, items);
    } else if let Some(description) = data["description"].as_str() {
        let _ = writeln!(
            out,
            "# {}\n\n{}\n\n{}",
            text(data, "title"),
            description,
            text(data, "url")
        );
    } else if let Some(raw) = data.as_str() {
        out.push_str(raw);
    } else {
        let _ = writeln!(
            out,
            "```json\n{}\n```",
            serde_json::to_string_pretty(data).expect("JSON value serializes")
        );
    }
    out
}

pub(crate) fn text<'a>(item: &'a Value, key: &str) -> &'a str {
    item[key].as_str().unwrap_or_default()
}

fn number(item: &Value, key: &str) -> String {
    item[key]
        .as_i64()
        .map_or_else(|| "?".into(), |value| value.to_string())
}

fn date(item: &Value) -> &str {
    let date = text(item, "created");
    date.get(..10).unwrap_or(date)
}

fn render_listing(out: &mut String, items: &[Value]) {
    for (index, item) in items.iter().enumerate() {
        let number_in_list = index + 1;
        match item["kind"].as_str() {
            Some("post") => {
                let _ = writeln!(
                    out,
                    "{number_in_list}. **{}** · r/{} · u/{} · {} pts · {} comments · {}",
                    text(item, "title"),
                    text(item, "subreddit"),
                    text(item, "author"),
                    number(item, "score"),
                    number(item, "num_comments"),
                    date(item)
                );
                let _ = writeln!(out, "   {}", text(item, "permalink"));
                let excerpt: String = text(item, "selftext").chars().take(280).collect();
                for line in excerpt.lines() {
                    let _ = writeln!(out, "   {line}");
                }
                if let Some(comments) = item["top_comments"].as_array() {
                    let mut body = String::new();
                    render_comments(&mut body, comments);
                    for line in body.lines() {
                        let _ = writeln!(out, "   {line}");
                    }
                }
            }
            Some("comment") => {
                let _ = writeln!(
                    out,
                    "{number_in_list}. **u/{}** · r/{} · {} pts · {}",
                    text(item, "author"),
                    text(item, "subreddit"),
                    number(item, "score"),
                    date(item)
                );
                let _ = writeln!(out, "   {}", text(item, "permalink"));
                for line in text(item, "body").lines() {
                    let _ = writeln!(out, "   {line}");
                }
            }
            _ => {
                let _ = write!(out, "{number_in_list}. **{}**", text(item, "name"));
                for key in ["title", "description", "url"] {
                    if let Some(value) = item[key].as_str().filter(|value| !value.is_empty()) {
                        let _ = write!(out, " · {value}");
                    }
                }
                if let Some(karma) = item["total_karma"].as_i64() {
                    let _ = write!(out, " · {karma} karma");
                }
                out.push('\n');
            }
        }
    }
}

fn render_comments(out: &mut String, comments: &[Value]) {
    let mut stack: Vec<_> = comments.iter().rev().map(|comment| (comment, 0)).collect();
    while let Some((comment, depth)) = stack.pop() {
        let indent = "  ".repeat(depth);
        let op = if comment["is_submitter"].as_bool().unwrap_or_default() {
            " (OP)"
        } else {
            ""
        };
        let _ = writeln!(
            out,
            "{indent}- **u/{}**{op} · {} pts · {}",
            text(comment, "author"),
            number(comment, "score"),
            date(comment)
        );
        for line in text(comment, "body").lines() {
            let _ = writeln!(out, "{indent}  {line}");
        }
        if let Some(replies) = comment["replies"].as_array() {
            stack.extend(replies.iter().rev().map(|reply| (reply, depth + 1)));
        }
    }
}
