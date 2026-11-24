use super::{
    LiveBackend,
    parse::{self, Comments, More},
};
use crate::{
    backend::{CommentId, ThreadMeta, ThreadQuery},
    error::AppError,
    http::THREAD_TTL,
    model::{Comment, Thread, Unresolved},
    tree,
};
use std::collections::{HashSet, VecDeque};

const BATCH_SIZE: usize = 100;
const FULL_LIMIT: &str = "500";

pub(super) fn thread(
    backend: &LiveBackend,
    query: &ThreadQuery,
) -> Result<(Thread, ThreadMeta), AppError> {
    if query.full && query.max_requests == 0 {
        return Err(AppError::usage("--full requires at least one request"));
    }
    let path = format!("/comments/{}", query.post);
    let sort = query.sort.as_str();
    let limit = query.limit.to_string();
    let depth = query.depth.map(|depth| depth.to_string());
    let mut params = vec![
        ("sort", sort),
        ("limit", if query.full { FULL_LIMIT } else { &limit }),
    ];
    if !query.full
        && let Some(depth) = depth.as_deref()
    {
        params.push(("depth", depth));
    }
    if let Some(comment) = &query.comment {
        params.push(("comment", comment.as_str()));
    }
    let (post, initial) = parse::thread(&backend.get(&path, &params, THREAD_TTL)?)?;
    let link_id = format!("t3_{}", query.post);
    let mut requests = 1;
    let (flat, unresolved, complete, more_count) = if query.full {
        let mut expansion = Expansion::default();
        if let Some(parent) = query.comment.clone() {
            expansion.scheduled.insert(Work::Continue(parent.clone()));
            expansion.add_continuation(parent, initial);
        } else {
            expansion.add(initial);
        }
        while requests < query.max_requests {
            let Some(request) = expansion.next_request() else {
                break;
            };
            requests += 1;
            let fetched = match &request {
                Request::Children(ids) => {
                    let children = ids
                        .iter()
                        .map(CommentId::as_str)
                        .collect::<Vec<_>>()
                        .join(",");
                    backend
                        .get(
                            "/api/morechildren",
                            &[
                                ("api_type", "json"),
                                ("link_id", &link_id),
                                ("children", &children),
                                ("sort", sort),
                            ],
                            THREAD_TTL,
                        )
                        .and_then(|body| parse::morechildren(&body))
                }
                Request::Continue(parent) => backend
                    .get(
                        &path,
                        &[
                            ("comment", parent.as_str()),
                            ("limit", FULL_LIMIT),
                            ("sort", sort),
                        ],
                        THREAD_TTL,
                    )
                    .and_then(|body| parse::thread(&body).map(|(_, continuation)| continuation)),
            };
            let Ok(parsed) = fetched else {
                expansion.stop(request);
                break;
            };
            match request {
                Request::Children(ids) => {
                    expansion.add(parsed);
                    expansion.note_missing(ids);
                }
                Request::Continue(parent) => expansion.add_continuation(parent, parsed),
            }
        }
        let (comments, unresolved) = expansion.finish();
        let complete = unresolved.is_empty();
        (comments, unresolved, complete, None)
    } else {
        // A stub's count includes descendants whose IDs Reddit hasn't listed.
        let more_count = initial
            .more
            .iter()
            .map(|stub| match stub {
                More::Children(ids) => ids.len(),
                More::Continue(_) => 0,
            })
            .sum();
        (
            initial.comments,
            Vec::new(),
            initial.more.is_empty(),
            Some(more_count),
        )
    };

    let comments = tree::rebuild(&post.fullname, flat);
    let mut thread = Thread {
        post,
        comments_returned: tree::count(&comments),
        comments,
        complete,
        unresolved,
    };
    let mut notes = Vec::new();
    if query.full {
        tree::prune_thread(&mut thread, None, query.depth, query.limit, &mut notes);
    }
    Ok((
        thread,
        ThreadMeta {
            more_count,
            requests: Some(requests),
            notes,
        },
    ))
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum Work {
    Child(CommentId),
    Continue(CommentId),
}

#[derive(Debug)]
enum Request {
    Children(Vec<CommentId>),
    Continue(CommentId),
}

#[derive(Default)]
struct Expansion {
    comments: Vec<Comment>,
    returned: HashSet<String>,
    queue: VecDeque<Work>,
    scheduled: HashSet<Work>,
    unresolved: Vec<(Work, &'static str)>,
}

impl Expansion {
    fn add(&mut self, parsed: Comments) {
        self.comments.extend(
            parsed
                .comments
                .into_iter()
                .filter(|comment| self.returned.insert(comment.id.clone())),
        );
        for stub in parsed.more {
            match stub {
                More::Children(children) => {
                    for id in children {
                        if !self.returned.contains(id.as_str()) {
                            self.enqueue(Work::Child(id));
                        }
                    }
                }
                More::Continue(id) => self.enqueue(Work::Continue(id)),
            }
        }
    }

    fn enqueue(&mut self, work: Work) {
        if self.scheduled.insert(work.clone()) {
            self.queue.push_back(work);
        }
    }

    fn next_request(&mut self) -> Option<Request> {
        let mut children = Vec::new();
        while children.len() < BATCH_SIZE {
            match self.queue.front() {
                Some(Work::Continue(_)) if !children.is_empty() => break,
                None => break,
                _ => {}
            }
            match self.queue.pop_front()? {
                Work::Child(id) if !self.returned.contains(id.as_str()) => children.push(id),
                Work::Child(_) => {}
                Work::Continue(id) => return Some(Request::Continue(id)),
            }
        }
        (!children.is_empty()).then_some(Request::Children(children))
    }

    fn note_missing(&mut self, requested: Vec<CommentId>) {
        for id in requested {
            if !self.returned.contains(id.as_str()) {
                self.unresolved.push((Work::Child(id), "not_returned"));
            }
        }
    }

    fn add_continuation(&mut self, parent: CommentId, parsed: Comments) {
        let fullname = format!("t1_{parent}");
        if parsed
            .more
            .iter()
            .any(|stub| matches!(stub, More::Continue(id) if *id == parent))
        {
            self.unresolved
                .push((Work::Continue(parent), "continuation_still_truncated"));
        } else if !parsed
            .comments
            .iter()
            .any(|comment| comment.id == parent.as_str() || comment.parent_id == fullname)
        {
            self.unresolved
                .push((Work::Continue(parent), "not_returned"));
        }
        self.add(parsed);
    }

    fn stop(&mut self, failed: Request) {
        match failed {
            Request::Children(ids) => {
                self.unresolved.extend(
                    ids.into_iter()
                        .map(|id| (Work::Child(id), "upstream_error")),
                );
            }
            Request::Continue(id) => self.unresolved.push((Work::Continue(id), "upstream_error")),
        }
        self.unresolved
            .extend(self.queue.drain(..).map(|work| (work, "upstream_error")));
    }

    fn finish(self) -> (Vec<Comment>, Vec<Unresolved>) {
        let unresolved = self
            .unresolved
            .into_iter()
            .chain(
                self.queue
                    .into_iter()
                    .map(|work| (work, "request_budget_exhausted")),
            )
            .filter_map(|(work, reason)| {
                let id = match work {
                    Work::Child(id) if self.returned.contains(id.as_str()) => return None,
                    Work::Child(id) | Work::Continue(id) => id,
                };
                Some(Unresolved {
                    id: id.as_str().into(),
                    reason: reason.into(),
                })
            })
            .collect();
        (self.comments, unresolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> CommentId {
        CommentId::parse(value).unwrap()
    }

    #[test]
    fn batches_cap_at_one_hundred_and_never_reschedule_an_omitted_id() {
        let ids = (0..101)
            .map(|index| id(&format!("id{index}")))
            .collect::<Vec<_>>();
        let mut expansion = Expansion::default();
        expansion.add(Comments {
            comments: vec![],
            more: vec![More::Children(ids.clone())],
        });
        let Some(Request::Children(first)) = expansion.next_request() else {
            panic!("expected a batch");
        };
        assert_eq!(first, ids[..100]);
        expansion.note_missing(first);
        expansion.add(Comments {
            comments: vec![Comment {
                id: "id1".into(),
                ..Comment::default()
            }],
            more: vec![More::Children(vec![id("id0"), id("id1")])],
        });
        let Some(Request::Children(last)) = expansion.next_request() else {
            panic!("expected remaining child");
        };
        assert_eq!(last, ids[100..]);
        assert!(expansion.next_request().is_none());
        let (_, unresolved) = expansion.finish();
        assert_eq!(unresolved.len(), 99);
        assert_eq!(unresolved[0].id, "id0");
        assert_eq!(unresolved[0].reason, "not_returned");
    }

    #[test]
    fn follows_each_continuation_once_and_tracks_leftover_work() {
        let mut expansion = Expansion::default();
        let parent = id("abc");
        expansion.add(Comments {
            comments: vec![],
            more: vec![
                More::Continue(parent.clone()),
                More::Continue(parent.clone()),
            ],
        });
        assert!(matches!(expansion.next_request(), Some(Request::Continue(id)) if id == parent));
        expansion.add_continuation(
            parent.clone(),
            Comments {
                comments: vec![],
                more: vec![More::Continue(parent), More::Children(vec![id("def")])],
            },
        );
        let (_, unresolved) = expansion.finish();
        assert_eq!(unresolved.len(), 2);
        assert_eq!(unresolved[0].id, "abc");
        assert_eq!(unresolved[0].reason, "continuation_still_truncated");
        assert_eq!(unresolved[1].id, "def");
        assert_eq!(unresolved[1].reason, "request_budget_exhausted");
    }
}
