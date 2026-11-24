use super::{Output, date_range};
use crate::{
    backend::{Backend, SourceKind, UserListing, UserQuery},
    cli::{Period, UserArgs, UserSort},
    error::AppError,
    ids::UserName,
    model::Item,
};
use serde_json::json;

pub fn run(backend: &dyn Backend, args: UserArgs) -> Result<Output, AppError> {
    let source = backend.kind();
    let name = UserName::parse(&args.name)?;
    let listing = if args.posts {
        UserListing::Posts
    } else if args.comments {
        UserListing::Comments
    } else {
        UserListing::Overview
    };
    if source == SourceKind::Live && (args.since.is_some() || args.until.is_some()) {
        return Err(source.unsupported("--since/--until"));
    }
    if source == SourceKind::Archive {
        if args.sort != UserSort::New {
            return Err(source.unsupported(format_args!("--sort {}", args.sort.as_str())));
        }
        if args.time.is_some() {
            return Err(source.unsupported("--time (use --since/--until)"));
        }
    }
    let (since, until) = date_range(args.since.as_deref(), args.until.as_deref())?;
    let query = UserQuery {
        name,
        listing,
        sort: args.sort,
        time: args.time.unwrap_or(Period::All),
        since,
        until,
        limit: args.limit,
        after: source.cursor(args.after)?,
    };
    let user = backend.user_about(&query.name)?;
    let mut page = backend.user_items(&query)?;
    for item in &mut page.items {
        if let Item::Comment(comment) = item {
            comment.replies.clear();
        }
    }
    let mut output = Output::page(page, source);
    output.data = json!({ "user": user, "items": output.data });
    Ok(output)
}
