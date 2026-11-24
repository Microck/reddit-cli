use super::{Output, single_subreddit};
use crate::{
    cli::CacheCommand,
    config::{Config, Paths},
    error::AppError,
    ids::{UserName, duration},
    store::Store,
};
use serde_json::json;

pub fn run(command: CacheCommand, paths: &Paths, config: &Config) -> Result<Output, AppError> {
    if matches!(command, CacheCommand::Clear) {
        Store::clear(paths)?;
        return Ok(Output::new(json!({ "ok": true }), "store"));
    }
    let retention = config.retention()?;
    let store = Store::open(paths, retention)?;
    let output = match command {
        CacheCommand::Search {
            query,
            sub,
            author,
            kind,
            limit,
        } => {
            let sub = sub.as_deref().map(single_subreddit).transpose()?;
            let author = author.as_deref().map(UserName::parse).transpose()?;
            let mut output = Output::new(
                store.search(
                    &query,
                    sub.as_ref().map(|name| name.as_str()),
                    author.as_ref().map(|name| name.as_str()),
                    kind.map(|kind| kind.as_str()),
                    limit,
                )?,
                "store",
            );
            output.meta.cached = true;
            output
        }
        CacheCommand::Stats => Output::new(store.stats()?, "store"),
        CacheCommand::Prune { older_than } => {
            let age = older_than
                .as_deref()
                .map(duration)
                .transpose()?
                .unwrap_or(retention);
            Output::new(store.prune(age)?, "store")
        }
        CacheCommand::Clear => unreachable!("clear bypasses schema initialization"),
    };
    Ok(output)
}
