use super::Output;
use crate::{
    backend::{Backend, SourceKind, ThreadQuery},
    cli::{ThreadArgs, ThreadSort},
    error::AppError,
    ids::{CommentId, thread_target},
};

pub fn run(backend: &dyn Backend, args: ThreadArgs) -> Result<Output, AppError> {
    let source = backend.kind();
    let (post, embedded_comment) = thread_target(&args.url_or_id)?;
    let comment = args
        .comment
        .as_deref()
        .map(CommentId::parse)
        .transpose()?
        .or(embedded_comment);
    let archive = source == SourceKind::Archive;
    let sort = args.sort.unwrap_or(if archive {
        ThreadSort::Top
    } else {
        ThreadSort::Confidence
    });
    if archive && !matches!(sort, ThreadSort::Top | ThreadSort::New | ThreadSort::Old) {
        return Err(source.unsupported(format_args!("--sort {}", sort.as_str())));
    }
    let max_requests = args.max_requests.unwrap_or(50);
    if !archive && args.full && max_requests == 0 {
        return Err(AppError::usage("--full requires at least one request"));
    }
    let query = ThreadQuery {
        post,
        comment,
        sort,
        depth: args.depth,
        limit: args.limit,
        full: args.full && !archive,
        max_requests,
    };
    let (thread, meta) = backend.thread(&query)?;
    let mut output = Output::new(thread, source.as_str());
    output.meta.more_count = meta.more_count;
    output.meta.requests = meta.requests;
    output.meta.notes = meta.notes;
    if archive && (args.full || args.max_requests.is_some()) {
        output
            .meta
            .notes
            .push("--full and --max-requests are ignored for archive threads.".into());
    }
    output.file = args.output;
    Ok(output)
}
