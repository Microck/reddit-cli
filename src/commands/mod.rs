mod auth;
mod cache;
mod search;
mod sub;
mod thread;
mod user;

pub use auth::run as auth;
pub use cache::run as cache;

use crate::{
    agent,
    backend::{Backend, SourceKind},
    cli::{Command, SearchType, SkillsCommand},
    error::{AppError, ErrorCode},
    ids::{SubName, date},
    model::{Meta, now, timestamp},
    store::Store,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::path::PathBuf;

pub struct Output {
    pub data: Value,
    pub meta: Meta,
    pub file: Option<PathBuf>,
}

impl Output {
    pub fn new(data: impl Serialize, source: &'static str) -> Self {
        Self {
            data: serde_json::to_value(data).expect("command payload serializes"),
            meta: Meta::new(source),
            file: None,
        }
    }

    pub fn page<T: Serialize>(page: crate::backend::Page<T>, source: SourceKind) -> Self {
        let mut output = Self::new(page.items, source.as_str());
        output.meta.after = page.after;
        output.meta.notes = page.notes;
        output
    }

    pub fn persist(
        &self,
        store: &Store,
        source: SourceKind,
        fetched_at: i64,
    ) -> Result<(), AppError> {
        store.upsert(&self.data, source, fetched_at)
    }

    pub fn set_fetched_at(&mut self, fetched_at: i64) {
        self.meta.fetched_at = timestamp(fetched_at);
    }
}

pub fn run_backend(backend: &dyn Backend, command: Command) -> Result<Output, AppError> {
    let source = backend.kind();
    // Archive caveats only where they apply: score freshness for commands that return
    // posts/comments, keyword-search semantics for post/comment search.
    let (score_note, search_note) = match &command {
        Command::Search(args) => {
            let content = matches!(args.search_type, SearchType::Posts | SearchType::Comments);
            (content, content)
        }
        Command::Sub(_) | Command::Thread(_) | Command::User(_) => (true, false),
        _ => (false, false),
    };
    let mut output = match command {
        Command::Sub(args) => sub::run(backend, args),
        Command::About { name } => sub::about(backend, &name),
        Command::Search(args) => search::run(backend, args),
        Command::Thread(args) => thread::run(backend, args),
        Command::User(args) => user::run(backend, args),
        _ => unreachable!("local commands are dispatched separately"),
    }
    .map_err(|error| {
        if error.hint.is_none() && !matches!(error.code, ErrorCode::EUsage | ErrorCode::EShareLink)
        {
            error.with_hint(format!("Try {}.", source.alternative()))
        } else {
            error
        }
    })?;
    if source == SourceKind::Archive {
        if score_note {
            output.meta.notes.push("Archive scores, upvote ratios, and comment counts are refreshed once, about 36 hours after posting. These fields are null until the refresh.".into());
        }
        if search_note {
            output.meta.notes.push("Archive search uses simple post keywords and PostgreSQL full-text search (websearch_to_tsquery syntax) for comments. Very active subreddits or users may time out.".into());
        }
    }
    Ok(output)
}

pub fn skills(command: SkillsCommand) -> Result<Output, AppError> {
    let data = match command {
        SkillsCommand::List => json!([{ "name": agent::NAME, "description": agent::DESCRIPTION }]),
        SkillsCommand::Get { name } if name == agent::NAME => json!(agent::SKILL),
        SkillsCommand::Get { .. } => {
            return Err(AppError::new(ErrorCode::ENotFound, "skill not found"));
        }
    };
    Ok(Output::new(data, "store"))
}

fn single_subreddit(name: &str) -> Result<SubName, AppError> {
    let name = SubName::parse(name)?;
    if name.is_multi() {
        return Err(AppError::usage("provide a single subreddit name"));
    }
    Ok(name)
}

fn date_range(
    since: Option<&str>,
    until: Option<&str>,
) -> Result<(Option<i64>, Option<i64>), AppError> {
    let now = now();
    let since = since.map(|value| date(value, now)).transpose()?;
    let until = until.map(|value| date(value, now)).transpose()?;
    if matches!((since, until), (Some(start), Some(end)) if start > end) {
        return Err(AppError::usage("--since must be at or before --until"));
    }
    Ok((since, until))
}
