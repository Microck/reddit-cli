use super::Output;
use crate::{
    backend::{Backend, SourceKind, SubQuery},
    cli::{SubArgs, SubSort},
    error::AppError,
    ids::SubName,
};

pub fn run(backend: &dyn Backend, args: SubArgs) -> Result<Output, AppError> {
    let source = backend.kind();
    let name = SubName::parse(&args.name)?;
    let sort = args.sort.unwrap_or(match source {
        SourceKind::Live => SubSort::Hot,
        SourceKind::Archive => SubSort::New,
    });
    if source == SourceKind::Archive {
        if name.is_multi() {
            return Err(source.unsupported("multireddits"));
        }
        if sort != SubSort::New {
            return Err(source.unsupported(format_args!(
                "--sort {} (archive accepts new)",
                sort.as_str()
            )));
        }
        if args.time.is_some() {
            return Err(source.unsupported("--time"));
        }
    }
    let query = SubQuery {
        name,
        sort,
        time: args.time,
        limit: args.limit,
        after: source.cursor(args.after)?,
    };
    Ok(Output::page(backend.subreddit_posts(&query)?, source))
}

pub fn about(backend: &dyn Backend, name: &str) -> Result<Output, AppError> {
    let source = backend.kind();
    let name = SubName::parse(name)?;
    if source == SourceKind::Archive && name.is_multi() {
        return Err(source.unsupported("multireddits"));
    }
    Ok(Output::new(
        backend.subreddit_about(&name)?,
        source.as_str(),
    ))
}
