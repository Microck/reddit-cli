use crate::error::{AppError, ErrorCode};
use chrono::NaiveDate;
use reqwest::Url;
use std::fmt;

macro_rules! validated_type {
    ($name:ident, $parse:ident) => {
        #[derive(Clone, Debug, Eq, Hash, PartialEq)]
        pub struct $name(String);

        impl $name {
            pub fn parse(raw: &str) -> Result<Self, AppError> {
                $parse(raw).map(Self)
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

validated_type!(SubName, sub_name);
validated_type!(UserName, user_name);
validated_type!(PostId, post_id);
validated_type!(CommentId, comment_id);

fn sub_name(raw: &str) -> Result<String, AppError> {
    let name = strip_prefix(raw, &["/r/", "r/"]);
    if name.split('+').all(|part| {
        valid_name(part, 2, 21, false)
            || part
                .strip_prefix("u_")
                .is_some_and(|user| valid_name(user, 3, 20, true))
    }) {
        Ok(name.into())
    } else {
        Err(AppError::usage("invalid subreddit name"))
    }
}

impl SubName {
    pub fn is_multi(&self) -> bool {
        self.0.contains('+')
    }
}

fn user_name(raw: &str) -> Result<String, AppError> {
    let name = strip_prefix(raw, &["/u/", "u/", "/user/", "user/"]);
    if valid_name(name, 3, 20, true) {
        Ok(name.into())
    } else {
        Err(AppError::usage("invalid username"))
    }
}

fn strip_prefix<'a>(raw: &'a str, prefixes: &[&str]) -> &'a str {
    prefixes
        .iter()
        .find_map(|prefix| raw.strip_prefix(prefix))
        .unwrap_or(raw)
}

fn valid_name(name: &str, min: usize, max: usize, hyphen: bool) -> bool {
    (min..=max).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || (hyphen && byte == b'-'))
}

fn base36(raw: &str, prefix: &str) -> Result<String, AppError> {
    let lower = raw.to_ascii_lowercase();
    // A fullname of the other kind still contains an underscore and fails validation.
    let value = lower.strip_prefix(prefix).unwrap_or(&lower);
    if (2..=13).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        Ok(value.into())
    } else {
        Err(AppError::usage("invalid Reddit ID"))
    }
}

fn post_id(raw: &str) -> Result<String, AppError> {
    base36(raw, "t3_")
}

fn comment_id(raw: &str) -> Result<String, AppError> {
    base36(raw, "t1_")
}

pub fn thread_target(raw: &str) -> Result<(PostId, Option<CommentId>), AppError> {
    let bare = raw.split(['?', '#']).next().unwrap_or(raw);
    let bare = bare.strip_suffix(".json").unwrap_or(bare);
    if !bare.contains('/') {
        return Ok((PostId::parse(bare)?, None));
    }

    let url = if raw.starts_with('/') {
        Url::parse(&format!("https://www.reddit.com{raw}"))
    } else if raw.contains("://") {
        Url::parse(raw)
    } else {
        Url::parse(&format!("https://{raw}"))
    }
    .map_err(|_| AppError::usage("invalid Reddit URL"))?;
    let host = url.host_str().unwrap_or_default();
    let path = url.path().trim_end_matches('/');
    let path = path.strip_suffix(".json").unwrap_or(path);
    let parts: Vec<_> = path.trim_start_matches('/').split('/').collect();
    let invalid = || AppError::usage("invalid Reddit thread URL");
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(invalid());
    }
    if host == "redd.it" {
        return match parts.as_slice() {
            [id] => Ok((PostId::parse(id)?, None)),
            _ => Err(invalid()),
        };
    }
    if host != "reddit.com" && !host.ends_with(".reddit.com") {
        return Err(invalid());
    }
    let path = match parts.as_slice() {
        ["r", name, "s", _] => {
            SubName::parse(name)?;
            return Err(
                AppError::new(ErrorCode::EShareLink, "cannot resolve a Reddit share link")
                    .with_hint("Open the link in a browser and paste the canonical thread URL."),
            );
        }
        ["r", name, rest @ ..] => {
            SubName::parse(name)?;
            rest
        }
        ["user", name, rest @ ..] => {
            UserName::parse(name)?;
            rest
        }
        _ => parts.as_slice(),
    };
    match path {
        ["comments", id] | ["gallery", id] | ["comments", id, _] => Ok((PostId::parse(id)?, None)),
        ["comments", id, _, comment] => Ok((PostId::parse(id)?, Some(CommentId::parse(comment)?))),
        _ => Err(invalid()),
    }
}

pub fn duration(raw: &str) -> Result<i64, AppError> {
    let (amount, unit) = raw.split_at(raw.char_indices().last().map_or(0, |(index, _)| index));
    let seconds = match unit {
        "s" => 1,
        "m" => 30 * 86_400,
        "h" => 3_600,
        "d" => 86_400,
        "w" => 7 * 86_400,
        "y" => 365 * 86_400,
        _ => 0,
    };
    amount
        .parse::<i64>()
        .ok()
        .filter(|amount| *amount >= 0 && seconds > 0)
        .and_then(|amount| amount.checked_mul(seconds))
        .ok_or_else(|| AppError::usage("invalid duration (use a value such as 30d or 12h)"))
}

pub fn date(raw: &str, now: i64) -> Result<i64, AppError> {
    if let Ok(epoch) = raw.parse::<i64>() {
        return Ok(epoch);
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Ok(dt.timestamp());
    }
    if let Ok(day) = NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
        return Ok(day.and_time(chrono::NaiveTime::MIN).and_utc().timestamp());
    }
    duration(raw)
        .ok()
        .and_then(|span| now.checked_sub(span))
        .ok_or_else(|| {
            AppError::usage(
                "invalid date (use YYYY-MM-DD, RFC3339, epoch seconds or a relative duration)",
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_names_and_extracts_ids_without_trusting_host_or_path() {
        assert_eq!(
            SubName::parse("/r/rust+u_some-user").unwrap().as_str(),
            "rust+u_some-user"
        );
        assert!(SubName::parse("rust?x=1").is_err());
        assert!(UserName::parse("foo/bar").is_err());
        assert!(PostId::parse("t1_ab").is_err());
        let (post, comment) =
            thread_target("https://old.reddit.com/r/rust/comments/AB/a/Cd.json?q=1").unwrap();
        assert_eq!(post.as_str(), "ab");
        assert_eq!(comment.unwrap().as_str(), "cd");
        assert_eq!(thread_target("redd.it/ab").unwrap().0.as_str(), "ab");
        assert!(thread_target("https://reddit.com.example/comments/ab").is_err());
        assert_eq!(
            thread_target("/r/rust/s/abc").unwrap_err().code,
            ErrorCode::EShareLink
        );
    }

    #[test]
    fn normalizes_dates() {
        assert_eq!(date("1970-01-02", 0).unwrap(), 86_400);
        assert_eq!(date("1970-01-02T01:00:00+01:00", 0).unwrap(), 86_400);
        assert_eq!(date("-1000", 0).unwrap(), -1000);
        assert_eq!(date("6m", 180 * 86_400).unwrap(), 0);
        assert!(date("12🦀", 0).is_err());
    }
}
