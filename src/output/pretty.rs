use super::markdown;
use serde_json::Value;
use std::fmt::Write;

pub fn render(data: &Value, color: bool) -> String {
    if data.is_string() {
        return markdown::render(data);
    }
    if data.is_array()
        || data.get("post").is_some()
        || data.get("user").is_some()
        || data.get("description").is_some()
    {
        let mut text = markdown::render(data);
        if color && !text.is_empty() {
            text.insert_str(text.find('\n').unwrap_or(text.len()), "\x1b[0m");
            text.insert_str(0, "\x1b[1m");
        }
        return text;
    }
    let mut out = String::new();
    if let Some(fields) = data.as_object() {
        for (field, value) in fields {
            let _ = writeln!(
                out,
                "{}  {}",
                field.replace('_', " "),
                value
                    .as_str()
                    .map_or_else(|| value.to_string(), str::to_owned)
            );
        }
    }
    out
}
