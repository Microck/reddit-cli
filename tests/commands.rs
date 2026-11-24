use clap::Parser;
use reddit_research::{
    backend::{
        Backend, Page, PostId, SearchQuery, SourceKind, SubName, SubQuery, ThreadMeta, ThreadQuery,
        UserName, UserQuery,
    },
    cli::{Cli, Format},
    commands,
    error::{AppError, ErrorCode},
    model::{Comment, Item, Post, Subreddit, Thread, User, UserSummary, now, timestamp},
    output,
    store::Store,
};
use std::{fs, sync::Mutex};

#[derive(Default)]
struct FixtureBackend {
    searches: Mutex<Vec<(String, usize)>>,
    attached: Mutex<Vec<(String, usize)>>,
}

fn post(id: &str, title: &str, score: i64, parent: Option<&str>) -> Post {
    Post {
        id: id.into(),
        fullname: format!("t3_{id}"),
        subreddit: "rust".into(),
        author: "fixture-user".into(),
        title: title.into(),
        created: timestamp(0),
        score: Some(score),
        crosspost_parent: parent.map(str::to_owned),
        permalink: format!("https://www.reddit.com/comments/{id}"),
        ..Post::default()
    }
}

fn comment(id: &str, post: &str) -> Comment {
    Comment {
        id: id.into(),
        fullname: format!("t1_{id}"),
        link_id: format!("t3_{post}"),
        parent_id: format!("t3_{post}"),
        subreddit: "rust".into(),
        author: "fixture-user".into(),
        body: "Fixture comment text".into(),
        created: timestamp(0),
        ..Comment::default()
    }
}

impl Backend for FixtureBackend {
    fn kind(&self) -> SourceKind {
        SourceKind::Live
    }

    fn subreddit_posts(&self, _: &SubQuery) -> Result<Page<Post>, AppError> {
        unreachable!()
    }
    fn subreddit_about(&self, _: &SubName) -> Result<Subreddit, AppError> {
        unreachable!()
    }
    fn search_comments(&self, _: &SearchQuery) -> Result<Page<Comment>, AppError> {
        unreachable!()
    }
    fn search_subreddits(&self, _: &SearchQuery) -> Result<Page<Subreddit>, AppError> {
        unreachable!()
    }
    fn search_users(&self, _: &SearchQuery) -> Result<Page<UserSummary>, AppError> {
        unreachable!()
    }
    fn user_about(&self, name: &UserName) -> Result<User, AppError> {
        Ok(User {
            name: name.as_str().into(),
            ..User::default()
        })
    }
    fn user_items(&self, query: &UserQuery) -> Result<Page<Item>, AppError> {
        use reddit_research::backend::UserListing;
        let items = match query.listing {
            UserListing::Posts => vec![Item::Post(post("ab", "Fixture user post", 1, None))],
            UserListing::Comments => vec![Item::Comment(comment("cd", "ab"))],
            UserListing::Overview => vec![
                Item::Post(post("ab", "Fixture user post", 1, None)),
                Item::Comment(comment("cd", "ab")),
            ],
        };
        Ok(Page {
            items,
            after: None,
            notes: vec![],
        })
    }

    fn search_posts(&self, query: &SearchQuery) -> Result<Page<Post>, AppError> {
        let sub = query.sub.as_ref().unwrap().as_str();
        self.searches
            .lock()
            .unwrap()
            .push((sub.into(), query.limit));
        let items = match sub {
            "rust" => vec![
                post("aa", "Original release", 1, None),
                post("bb", "Useful release", 2, None),
            ],
            "linux" => vec![
                post("cc", "Crossposted release", 3, Some("t3_aa")),
                post("bb", "Useful release", 2, None),
            ],
            _ => {
                return Err(AppError::new(
                    ErrorCode::ENotFound,
                    "fixture subreddit missing",
                ));
            }
        };
        Ok(Page {
            items,
            after: Some("t3_last".into()),
            notes: vec![],
        })
    }

    fn top_comments(&self, post: &PostId, k: usize) -> Result<Page<Comment>, AppError> {
        self.attached
            .lock()
            .unwrap()
            .push((post.as_str().into(), k));
        Ok(Page {
            items: vec![comment(&format!("c{}", post.as_str()), post.as_str())],
            after: None,
            notes: Vec::new(),
        })
    }

    fn thread(&self, query: &ThreadQuery) -> Result<(Thread, ThreadMeta), AppError> {
        assert_eq!(query.post.as_str(), "ab");
        assert_eq!(query.comment.as_ref().unwrap().as_str(), "cd");
        let mut reply = comment("ef", "ab");
        reply.parent_id = "t1_cd".into();
        reply.depth = 1;
        let mut root = comment("cd", "ab");
        root.replies.push(reply);
        Ok((
            Thread {
                post: post("ab", "Fixture thread", 3, None),
                comments: vec![root],
                complete: true,
                comments_returned: 2,
                ..Thread::default()
            },
            ThreadMeta {
                requests: Some(1),
                ..ThreadMeta::default()
            },
        ))
    }
}

#[test]
fn multi_sub_search_ranks_then_attaches_comments_and_stores_each_returned_item() {
    let backend = FixtureBackend::default();
    let cli = Cli::try_parse_from([
        "reddit",
        "search",
        "release",
        "-s",
        "rust",
        "-s",
        "linux",
        "-s",
        "missing",
        "--rank",
        "score",
        "--limit",
        "2",
        "--comments",
        "1",
    ])
    .unwrap();
    let output = commands::run_backend(&backend, cli.command).unwrap();
    let store = Store::in_memory().unwrap();
    output.persist(&store, backend.kind(), now()).unwrap();

    let mut searches = backend.searches.into_inner().unwrap();
    searches.sort();
    assert_eq!(
        searches,
        [
            ("linux".into(), 2),
            ("missing".into(), 2),
            ("rust".into(), 2)
        ]
    );
    let mut attached = backend.attached.into_inner().unwrap();
    attached.sort();
    assert_eq!(attached, [("bb".into(), 1), ("cc".into(), 1)]);
    assert_eq!(output.data[0]["id"], "cc");
    assert_eq!(output.data[1]["id"], "bb");
    assert_eq!(output.data[0]["top_comments"][0]["id"], "ccc");
    assert!(output.meta.after.is_none());
    assert!(output.meta.notes[0].contains("r/missing"));
    assert_eq!(store.stats().unwrap().posts, 2);
    assert_eq!(store.stats().unwrap().comments, 2);
    assert!(
        store
            .search("Original", None, None, None, 5)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn thread_output_uses_file_extension_and_writes_nested_comments_to_the_store() {
    let backend = FixtureBackend::default();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("thread.jsonl");
    let cli = Cli::try_parse_from([
        "reddit",
        "thread",
        "/r/rust/comments/ab/title/cd",
        "-o",
        path.to_str().unwrap(),
    ])
    .unwrap();
    let output = commands::run_backend(&backend, cli.command).unwrap();
    let store = Store::in_memory().unwrap();
    output.persist(&store, backend.kind(), now()).unwrap();
    let ack = output::write(&output, Format::Json, false).unwrap();
    let ack: serde_json::Value = serde_json::from_str(&ack).unwrap();
    let contents = fs::read_to_string(&path).unwrap();
    assert_eq!(ack["data"]["path"], path.to_str().unwrap());
    assert_eq!(ack["data"]["bytes"], contents.len());
    let lines: Vec<serde_json::Value> = contents
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["kind"], "post");
    assert_eq!(lines[1]["id"], "cd");
    assert_eq!(lines[2]["id"], "ef");
    assert_eq!(lines[2]["depth"], 1);
    assert_eq!(lines[1]["replies"], serde_json::json!([]));
    assert_eq!(store.stats().unwrap().comments, 2);
}

#[test]
fn every_user_listing_includes_the_profile_and_tagged_items() {
    let backend = FixtureBackend::default();
    for (flag, kinds) in [
        ("--posts", &["post"][..]),
        ("--comments", &["comment"][..]),
        ("--overview", &["post", "comment"][..]),
    ] {
        let cli = Cli::try_parse_from(["reddit", "user", "u/fixture-user", flag]).unwrap();
        let output = commands::run_backend(&backend, cli.command).unwrap();
        assert_eq!(output.data["user"]["name"], "fixture-user");
        let actual: Vec<_> = output.data["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["kind"].as_str().unwrap())
            .collect();
        assert_eq!(actual, kinds);
    }
}
