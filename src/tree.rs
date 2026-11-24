use crate::{
    cli::ThreadSort,
    model::{Comment, Thread},
};
use std::{cmp::Reverse, collections::HashMap};

/// Rebuild from flat comments in backend order. Missing parents become roots.
/// A malformed cycle is cut at the first member encountered in backend order.
pub fn rebuild(root: &str, flat: Vec<Comment>) -> Vec<Comment> {
    let mut indices = HashMap::new();
    let mut nodes = Vec::new();
    for mut comment in flat {
        if indices.contains_key(&comment.fullname) {
            continue;
        }
        indices.insert(comment.fullname.clone(), nodes.len());
        comment.replies.clear();
        nodes.push(Some(comment));
    }
    let mut roots = Vec::new();
    let mut children = vec![Vec::new(); nodes.len()];
    for (index, node) in nodes.iter().enumerate() {
        let parent = &node.as_ref().expect("node is present").parent_id;
        match indices.get(parent).filter(|_| parent != root) {
            Some(&index_of_parent) if index_of_parent != index => {
                children[index_of_parent].push(index)
            }
            _ => roots.push(index),
        }
    }
    let mut forest = Vec::new();
    let mut visiting = vec![false; nodes.len()];
    // Walk roots first, then any cyclic components. An exit frame assembles the
    // node only after its children have been assembled.
    for root in roots.into_iter().chain(0..nodes.len()) {
        if visiting[root] {
            continue;
        }
        let mut stack = vec![(root, 0, false)];
        let mut built = Vec::new();
        while let Some((index, depth, exiting)) = stack.pop() {
            if exiting {
                let mut node = nodes[index].take().expect("node is assembled once");
                node.depth = depth;
                while built
                    .last()
                    .is_some_and(|child: &Comment| child.depth > depth)
                {
                    node.replies.push(built.pop().expect("child exists"));
                }
                node.replies.reverse();
                built.push(node);
            } else if !std::mem::replace(&mut visiting[index], true) {
                stack.push((index, depth, true));
                stack.extend(
                    children[index]
                        .iter()
                        .rev()
                        .map(|&child| (child, depth + 1, false)),
                );
            }
        }
        forest.extend(built);
    }
    forest
}

pub fn count(comments: &[Comment]) -> usize {
    let mut pending: Vec<_> = comments.iter().collect();
    let mut count = 0;
    while let Some(comment) = pending.pop() {
        count += 1;
        pending.extend(&comment.replies);
    }
    count
}

pub fn prune_thread(
    thread: &mut Thread,
    sort: Option<ThreadSort>,
    depth: Option<usize>,
    limit: usize,
    notes: &mut Vec<String>,
) {
    let kept = sort_prune(&mut thread.comments, sort, depth, limit);
    let dropped = thread.comments_returned - kept;
    thread.comments_returned = kept;
    if dropped > 0 {
        thread.complete = false;
        notes.push(format!("Thread settings dropped {dropped} comments."));
    }
}

/// Apply local ordering within sibling groups when requested, then retain
/// the first `limit` comments in preorder. Root comments have depth zero.
fn sort_prune(
    comments: &mut Vec<Comment>,
    sort: Option<ThreadSort>,
    depth: Option<usize>,
    limit: usize,
) -> usize {
    let mut remaining = limit;
    prune(comments, sort, depth.unwrap_or(usize::MAX), &mut remaining);
    limit - remaining
}

fn prune(
    comments: &mut Vec<Comment>,
    sort: Option<ThreadSort>,
    depth: usize,
    remaining: &mut usize,
) {
    match sort {
        Some(ThreadSort::Top) => comments.sort_by_key(|comment| Reverse(comment.score)),
        Some(ThreadSort::New) => comments.sort_by_key(|comment| Reverse(comment.created_utc)),
        Some(ThreadSort::Old) => comments.sort_by_key(|comment| comment.created_utc),
        _ => {}
    }
    comments.truncate(*remaining);
    let mut kept = 0;
    for comment in comments.iter_mut() {
        if *remaining == 0 {
            break;
        }
        kept += 1;
        *remaining -= 1;
        if depth == 0 {
            comment.replies.clear();
        } else if !comment.replies.is_empty() {
            prune(&mut comment.replies, sort, depth - 1, remaining);
        }
    }
    comments.truncate(kept);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comment(id: &str, parent: &str, score: i64) -> Comment {
        Comment {
            fullname: id.into(),
            parent_id: parent.into(),
            depth: 999,
            score: Some(score),
            ..Comment::default()
        }
    }

    #[test]
    fn rebuilds_out_of_order_and_prunes_by_preorder() {
        let mut comments = rebuild(
            "t3_post",
            vec![
                comment("t1_child", "t1_root", 1),
                comment("t1_other", "t3_post", 3),
                comment("t1_root", "t3_post", 4),
            ],
        );
        assert_eq!(comments[1].replies[0].depth, 1);
        assert_eq!(sort_prune(&mut comments, Some(ThreadSort::Top), None, 2), 2);
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].fullname, "t1_root");
        assert_eq!(comments[0].depth, 0);
    }

    #[test]
    fn keeps_orphans_and_breaks_cycles() {
        let mut comments = rebuild(
            "t3_post",
            vec![
                comment("t1_a", "t1_b", 1),
                comment("t1_b", "t1_a", 2),
                comment("t1_c", "t1_missing", 3),
            ],
        );
        assert_eq!(comments[0].fullname, "t1_c");
        assert_eq!(comments[1].replies[0].fullname, "t1_b");
        assert_eq!(
            sort_prune(&mut comments, Some(ThreadSort::Old), Some(0), 10),
            2
        );
        assert!(comments.iter().all(|comment| comment.replies.is_empty()));
    }
}
