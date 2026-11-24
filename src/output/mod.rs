mod markdown;
mod pretty;

use crate::{cli::Format, commands::Output, config::file_error, error::AppError, model::Envelope};
use serde::Serialize;
use serde_json::json;
use std::{io::Write, path::Path};

pub fn render(output: &Output, format: Format, color: bool) -> String {
    match format {
        Format::Json => line(&Envelope {
            data: &output.data,
            meta: &output.meta,
        }),
        Format::Jsonl => jsonl(output),
        Format::Md => markdown::render(&output.data),
        Format::Pretty => pretty::render(&output.data, color),
        Format::Auto => unreachable!("format is resolved before rendering"),
    }
}

pub fn write(output: &Output, format: Format, color: bool) -> Result<String, AppError> {
    let Some(path) = &output.file else {
        return Ok(render(output, format, color));
    };
    let file_format = match path.extension().and_then(|extension| extension.to_str()) {
        Some("md") => Format::Md,
        Some("json") => Format::Json,
        Some("jsonl") => Format::Jsonl,
        _ => format,
    };
    let content = render(output, file_format, false);
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file =
        tempfile::NamedTempFile::new_in(parent).map_err(|error| file_error(path, error))?;
    file.write_all(content.as_bytes())
        .map_err(|error| file_error(path, error))?;
    file.persist(path)
        .map_err(|error| file_error(path, error.error))?;
    Ok(match format {
        Format::Json | Format::Jsonl => {
            line(&json!({ "data": { "path": path, "bytes": content.len() } }))
        }
        _ => format!("Wrote {} bytes to {}\n", content.len(), path.display()),
    })
}

fn line(value: &impl Serialize) -> String {
    let mut text = serde_json::to_string(value).expect("output serializes");
    text.push('\n');
    text
}

fn jsonl(output: &Output) -> String {
    if let Some(items) = output.data.as_array() {
        items.iter().map(line).collect()
    } else if let (Some(post), Some(comments)) =
        (output.data.get("post"), output.data["comments"].as_array())
    {
        let mut text = line(post);
        let mut stack: Vec<_> = comments.iter().rev().collect();
        while let Some(comment) = stack.pop() {
            let mut flat = serde_json::Map::new();
            for (key, value) in comment.as_object().expect("comment is an object") {
                if key == "replies" {
                    if let Some(replies) = value.as_array() {
                        stack.extend(replies.iter().rev());
                    }
                    flat.insert(key.clone(), json!([]));
                } else {
                    flat.insert(key.clone(), value.clone());
                }
            }
            text.push_str(&line(&flat));
        }
        text
    } else {
        render(output, Format::Json, false)
    }
}

pub fn render_error(error: &AppError, format: Format) -> String {
    match format {
        Format::Json | Format::Jsonl => line(&json!({ "error": error })),
        _ => {
            let mut text = format!("{error}\n");
            if let Some(hint) = &error.hint {
                text.push_str(&format!("hint: {hint}\n"));
            }
            text
        }
    }
}
