#[allow(dead_code)]
mod support;

use serde_json::{Value, json};
use std::process::Output;
use support::{Fixtures, Route, State};
use tiny_http::Method;

fn route(target: &str, fixture: &'static str) -> Route {
    Route::new(Method::Get, target, fixture, 200)
}

fn run(state: &State, fixtures: &Fixtures, args: &[&str]) -> Output {
    state
        .command()
        .env("REDDIT_CLI_ARCTIC_BASE", format!("{}/api", fixtures.base))
        .args(["--source", "archive", "--format", "json"])
        .args(args)
        .output()
        .unwrap()
}

fn success(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["meta"]["source"], "archive");
    assert!(output.stderr.is_empty());
    response
}

fn has_note(response: &Value, text: &str) -> bool {
    response["meta"]["notes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|note| note.as_str().unwrap().contains(text))
}

fn failure(output: Output, exit: i32, code: &str) -> Value {
    assert_eq!(
        output.status.code(),
        Some(exit),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let response: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(response["error"]["code"], code);
    response["error"].clone()
}

#[test]
fn subreddit_new_hides_placeholder_metrics_and_resumes_from_the_cursor() {
    let state = State::new();
    let fixtures = Fixtures::serve(vec![
        route(
            "/api/posts/search?subreddit=rust&limit=100&sort=desc",
            "archive/posts-sub-rust.json",
        ),
        route(
            "/api/posts/search?subreddit=rust&limit=100&sort=desc&before=1795530951",
            "archive/posts-search-rust.json",
        ),
    ]);
    let first = success(run(&state, &fixtures, &["sub", "r/rust", "--limit", "2"]));
    assert_eq!(first["data"][0]["fullname"], "t3_1y4euiy");
    assert_eq!(first["data"][0]["score"], Value::Null);
    assert_eq!(first["data"][0]["num_comments"], Value::Null);
    assert_eq!(first["data"][0]["upvote_ratio"], Value::Null);
    assert_eq!(first["data"][0]["media"]["type"], "image");
    let after = first["meta"]["after"].as_str().unwrap();
    assert_eq!(after, "a:1795530950:1y4etub");
    assert!(has_note(&first, "36 hours"));
    assert!(!has_note(&first, "full-text search"));

    let second = success(run(
        &state,
        &fixtures,
        &["sub", "rust", "--limit", "2", "--after", after],
    ));
    assert_eq!(second["data"][0]["id"], "1y41n3y");
    assert_eq!(second["data"][0]["score"], 0);
    for post in second["data"].as_array().unwrap() {
        assert!(post["created_utc"].as_i64().unwrap() < 1795530325);
    }
    assert_eq!(fixtures.requests().len(), 2);
}

#[test]
fn an_archive_list_rejects_live_sorts_and_foreign_cursors_before_http() {
    let state = State::new();
    let fixtures = Fixtures::serve(vec![]);
    let unsupported = failure(
        run(&state, &fixtures, &["sub", "rust", "--sort", "hot"]),
        2,
        "E_UNSUPPORTED",
    );
    assert!(
        unsupported["hint"]
            .as_str()
            .unwrap()
            .contains("--source live")
    );
    failure(
        run(
            &state,
            &fixtures,
            &["sub", "rust", "--sort", "new", "--after", "t3_abc"],
        ),
        2,
        "E_USAGE",
    );
    assert!(fixtures.requests().is_empty());
}

#[test]
fn exact_about_and_prefix_searches_return_normalized_profiles() {
    let state = State::new();
    let fixtures = Fixtures::serve(vec![
        route(
            "/api/subreddits/search?subreddit=RuSt",
            "archive/subreddit-rust.json",
        ),
        route(
            "/api/subreddits/search?subreddit=Ruby",
            "archive/subreddit-rust.json",
        ),
        route(
            "/api/subreddits/search?subreddit_prefix=rust&limit=3&sort_type=subscribers",
            "archive/subreddits-prefix.json",
        ),
        route(
            "/api/users/search?author_prefix=spez&limit=3&sort_type=total_karma",
            "archive/users-prefix.json",
        ),
    ]);
    let about = success(run(&state, &fixtures, &["about", "RuSt"]));
    assert_eq!(about["data"]["name"], "rust");
    assert_eq!(about["meta"]["notes"], json!([]));
    assert_eq!(about["data"]["subscribers"], 432309);
    assert_eq!(about["data"]["active_users"], Value::Null);
    assert_eq!(about["data"]["url"], "https://www.reddit.com/r/rust/");
    failure(run(&state, &fixtures, &["about", "Ruby"]), 4, "E_NOT_FOUND");

    let subs = success(run(
        &state,
        &fixtures,
        &["search", "rust", "--type", "subs", "--limit", "3"],
    ));
    assert_eq!(subs["data"].as_array().unwrap().len(), 3);
    assert_eq!(subs["meta"]["notes"], json!([]));
    assert_eq!(subs["meta"]["after"], Value::Null);
    assert!(
        subs["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|sub| sub["name"] == "RustConsole")
    );
    let users = success(run(
        &state,
        &fixtures,
        &["search", "spez", "--type", "users", "--limit", "3"],
    ));
    assert!(users["data"].as_array().unwrap().iter().any(|user| {
        user["name"] == "spez"
            && user["total_karma"] == 1499932
            && user["created_utc"] == 1119552314
    }));
    assert_eq!(users["meta"]["after"], Value::Null);
    assert!(has_note(&users, "creation dates"));
    assert!(!has_note(&users, "36 hours"));
    for kind in ["subs", "users"] {
        failure(
            run(
                &state,
                &fixtures,
                &["search", "rust", "--type", kind, "--after", "a:10:ab"],
            ),
            2,
            "E_UNSUPPORTED",
        );
    }
    assert_eq!(fixtures.requests().len(), 4);
}

#[test]
fn keyword_search_maps_post_and_comment_filters_and_normalizes_dates() {
    let state = State::new();
    let fixtures = Fixtures::serve(vec![
        route(
            "/api/posts/search?query=async&subreddit=rust&after=1793491200&limit=100&sort=desc",
            "archive/posts-search-rust.json",
        ),
        route(
            "/api/comments/search?body=lifetime&subreddit=rust&limit=100&sort=desc",
            "archive/comments-search-rust.json",
        ),
    ]);
    let posts = success(run(
        &state,
        &fixtures,
        &[
            "search",
            "async",
            "--sub",
            "rust",
            "--since",
            "2026-11-01",
            "--limit",
            "3",
        ],
    ));
    assert_eq!(posts["data"][0]["kind"], "post");
    assert!(has_note(&posts, "full-text search"));
    assert_eq!(posts["data"][0]["subreddit"], "rust");
    assert_eq!(posts["data"][1]["selftext"], "[removed]");
    let comments = success(run(
        &state,
        &fixtures,
        &[
            "search", "lifetime", "--type", "comments", "--sub", "rust", "--limit", "3",
        ],
    ));
    assert_eq!(comments["data"][0]["kind"], "comment");
    assert_eq!(comments["data"][0]["score"], Value::Null);
    assert_eq!(comments["data"][0]["replies"], json!([]));
    assert!(
        comments["data"][0]["permalink"]
            .as_str()
            .unwrap()
            .starts_with("https://www.reddit.com/r/rust/comments/")
    );
    assert_eq!(fixtures.requests().len(), 2);
}

#[test]
fn keyword_search_requires_a_subreddit_or_author_before_http() {
    let state = State::new();
    let fixtures = Fixtures::serve(vec![]);
    for kind in ["posts", "comments"] {
        let error = failure(
            run(&state, &fixtures, &["search", "lifetime", "--type", kind]),
            2,
            "E_USAGE",
        );
        assert!(
            error["message"]
                .as_str()
                .unwrap()
                .contains("--sub or --author")
        );
    }
    assert!(fixtures.requests().is_empty());
}

#[test]
fn thread_rebuilds_the_tree_sorts_it_and_prunes_depth() {
    let state = State::new();
    let fixtures = Fixtures::serve(vec![
        route("/api/posts/ids?ids=1y1v3iw", "archive/post-ids.json"),
        route(
            "/api/comments/tree?link_id=t3_1y1v3iw&limit=9999",
            "archive/comments-tree.json",
        ),
    ]);
    let response = success(run(
        &state,
        &fixtures,
        &[
            "thread",
            "t3_1y1v3iw",
            "--limit",
            "2",
            "--sort",
            "new",
            "--depth",
            "0",
            "--full",
        ],
    ));
    let thread = &response["data"];
    assert_eq!(thread["post"]["fullname"], "t3_1y1v3iw");
    assert_eq!(thread["post"]["state"], "removed");
    assert_eq!(thread["comments_returned"], 2);
    assert_eq!(thread["comments"][0]["id"], "19syx0131orhg");
    assert_eq!(thread["comments"][1]["id"], "19sywwtw4r9r7");
    assert_eq!(thread["comments"][0]["depth"], 0);
    assert_eq!(thread["comments"][0]["replies"], json!([]));
    assert_eq!(thread["complete"], false);
    assert!(has_note(&response, "dropped 4 comments"));
    assert_eq!(response["meta"]["requests"], 2);
    assert!(
        response["meta"]["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|note| {
                note.as_str()
                    .unwrap()
                    .contains("--full and --max-requests are ignored")
            })
    );
}

#[test]
fn user_overview_merges_posts_and_comments_before_cutting_the_page() {
    let state = State::new();
    let fixtures = Fixtures::serve(vec![
        route("/api/users/search?author=spez", "archive/user-spez.json"),
        route(
            "/api/posts/search?author=spez&limit=100&sort=desc",
            "archive/posts-author-spez.json",
        ),
        route(
            "/api/comments/search?author=spez&limit=100&sort=desc",
            "archive/comments-author-spez.json",
        ),
    ]);
    let response = success(run(&state, &fixtures, &["user", "u/spez", "--limit", "3"]));
    assert_eq!(response["data"]["user"]["link_karma"], 832984);
    assert_eq!(response["data"]["user"]["comment_karma"], 666948);
    assert_eq!(response["data"]["items"][0]["kind"], "comment");
    assert_eq!(response["data"]["items"][1]["id"], "19ruo2bw7d6ps");
    assert_eq!(response["data"]["items"][2]["kind"], "post");
    assert_eq!(response["meta"]["after"], "a:1793304517:1xiupm0");
}

#[test]
fn arctic_usage_errors_keep_the_upstream_text() {
    let state = State::new();
    let fixtures = Fixtures::serve(vec![Route::new(
        Method::Get,
        "/api/comments/search?body=lifetime&subreddit=rust&limit=100&sort=desc",
        "archive/error-body-needs-filter.json",
        400,
    )]);
    let error = failure(
        run(
            &state,
            &fixtures,
            &["search", "lifetime", "--type", "comments", "--sub", "rust"],
        ),
        2,
        "E_USAGE",
    );
    assert_eq!(
        error["message"],
        "'body' query parameter requires one of: author, subreddit, link_id, parent_id"
    );
    assert_eq!(error["http_status"], 400);
    assert_eq!(fixtures.requests().len(), 1);
}

#[test]
fn arctic_timeout_retries_once_and_returns_a_retryable_upstream_error() {
    let state = State::new();
    let timeout = || {
        Route::new(
            Method::Get,
            "/api/posts/search?subreddit=rust&query=async&limit=100&sort=desc",
            "archive/error-timeout.json",
            422,
        )
    };
    let fixtures = Fixtures::serve(vec![timeout(), timeout()]);
    let error = failure(
        run(&state, &fixtures, &["search", "async", "--sub", "rust"]),
        7,
        "E_UPSTREAM",
    );
    assert_eq!(error["http_status"], 422);
    assert_eq!(error["retryable"], true);
    assert_eq!(error["message"], "Timeout. Maybe slow down a bit");
    assert!(error["hint"].as_str().unwrap().contains("--since/--until"));
    assert_eq!(fixtures.requests().len(), 2);
}

#[test]
fn search_attaches_only_root_comments_and_ignores_placeholder_scores() {
    let state = State::new();
    let fixtures = Fixtures::serve(vec![
        route(
            "/api/posts/search?subreddit=rust&query=farrey&limit=100&sort=desc",
            "archive/post-ids.json",
        ),
        route(
            "/api/comments/search?link_id=t3_1y1v3iw&parent_id=&limit=100&sort=desc",
            "../fixtures-archive-extra/thread-root-comments.json",
        ),
    ]);
    let response = success(run(
        &state,
        &fixtures,
        &[
            "search",
            "farrey",
            "--sub",
            "rust",
            "--limit",
            "1",
            "--comments",
            "1",
        ],
    ));
    let comments = response["data"][0]["top_comments"].as_array().unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0]["parent_id"], "t3_1y1v3iw");
    assert_eq!(comments[0]["replies"], json!([]));
    // The largest raw score is a placeholder, so all four roots tie on score.
    assert_eq!(comments[0]["id"], "19syx0131orhg");
    assert_eq!(comments[0]["score"], Value::Null);
    assert_eq!(fixtures.requests().len(), 2);
}

#[test]
fn archive_sub_uses_new_without_flags_and_notes_follow_format() {
    let state = State::new();
    let fixtures = Fixtures::serve(vec![route(
        "/api/posts/search?subreddit=rust&limit=100&sort=desc",
        "archive/posts-sub-rust.json",
    )]);
    let response = success(run(&state, &fixtures, &["sub", "rust"]));
    assert_eq!(response["data"][0]["id"], "1y4euiy");
    assert!(has_note(&response, "36 hours"));

    for format in ["pretty", "md", "jsonl"] {
        let output = state
            .command()
            .env("REDDIT_CLI_ARCTIC_BASE", format!("{}/api", fixtures.base))
            .args([
                "--source",
                "archive",
                "--format",
                format,
                "--offline",
                "sub",
                "rust",
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stderr).contains("36 hours"),
            format != "jsonl"
        );
    }
    assert_eq!(fixtures.requests().len(), 1);
}

#[test]
fn archive_cursor_resumes_same_second_for_posts_comments_and_overview() {
    for (mode, args) in [
        ("posts", vec!["sub", "rust"]),
        (
            "comments",
            vec!["search", "life", "--type", "comments", "-s", "rust"],
        ),
        ("overview", vec!["user", "spez"]),
    ] {
        let state = State::new();
        let mut routes = Vec::new();
        if mode == "overview" {
            routes.push(route(
                "/api/users/search?author=spez",
                "archive/user-spez.json",
            ));
        }
        let row = |id: &str, second| json!({"id": id, "created_utc": second});
        let searches = match mode {
            "posts" => vec![(
                "posts/search?subreddit=rust",
                vec![row("cc", 20), row("dd", 20), row("bb", 20), row("aa", 19)],
            )],
            "comments" => vec![(
                "comments/search?body=life&subreddit=rust",
                vec![row("cc", 20), row("dd", 20), row("bb", 20), row("aa", 19)],
            )],
            _ => vec![
                (
                    "posts/search?author=spez",
                    vec![row("dd", 20), row("bb", 20)],
                ),
                (
                    "comments/search?author=spez",
                    vec![row("aa", 19), row("cc", 20)],
                ),
            ],
        };
        for (path, ties) in searches {
            let path = format!("/api/{path}&limit=100&sort=desc");
            let body = json!({"data": ties}).to_string();
            routes.push(Route::inline(Method::Get, &path, body.clone(), 200));
            routes.push(Route::inline(
                Method::Get,
                &format!("{path}&before=21"),
                body,
                200,
            ));
        }
        let fixtures = Fixtures::serve(routes);
        let run_page = |after: Option<&str>| {
            let mut args = args.clone();
            args.extend(["--limit", "2"]);
            if let Some(after) = after {
                args.extend(["--after", after]);
            }
            success(run(&state, &fixtures, &args))
        };
        let assert_ids = |response: &Value, expected: [&str; 2]| {
            let items = if mode == "overview" {
                &response["data"]["items"]
            } else {
                &response["data"]
            };
            let ids: Vec<_> = items
                .as_array()
                .unwrap()
                .iter()
                .map(|row| row["id"].as_str().unwrap())
                .collect();
            assert_eq!(ids, expected, "{mode}");
        };
        let first = run_page(None);
        assert_ids(&first, ["dd", "cc"]);
        let after = first["meta"]["after"].as_str().unwrap();
        assert_eq!(after, "a:20:cc");
        let second = run_page(Some(after));
        assert_ids(&second, ["bb", "aa"]);
        assert!(second["meta"]["after"].is_null());
    }
}

#[test]
fn archive_top_comments_stops_after_five_pages_and_notes_truncation() {
    let state = State::new();
    let mut routes = vec![route(
        "/api/posts/search?subreddit=rust&query=farrey&limit=100&sort=desc",
        "archive/post-ids.json",
    )];
    for page in 0..5 {
        let newest = 1000 - page * 99;
        let mut path =
            "/api/comments/search?link_id=t3_1y1v3iw&parent_id=&limit=100&sort=desc".to_owned();
        if page > 0 {
            path.push_str(&format!("&before={}", newest + 1));
        }
        let rows: Vec<_> = (newest - 99..=newest)
            .rev()
            .map(|second| {
                json!({
                    "id": format!("id{second}"),
                    "created_utc": second,
                    "link_id": "t3_1y1v3iw",
                    "parent_id": "t3_1y1v3iw",
                    "score": 1000 - second,
                    "_meta": {"retrieved_2nd_on": 2000},
                })
            })
            .collect();
        routes.push(Route::inline(
            Method::Get,
            &path,
            json!({"data": rows}).to_string(),
            200,
        ));
    }
    let fixtures = Fixtures::serve(routes);
    failure(
        run(
            &state,
            &fixtures,
            &["search", "farrey", "-s", "rust", "--comments", "101"],
        ),
        2,
        "E_USAGE",
    );
    let response = success(run(
        &state,
        &fixtures,
        &["search", "farrey", "-s", "rust", "--comments", "100"],
    ));
    let comments = response["data"][0]["top_comments"].as_array().unwrap();
    assert_eq!(comments.len(), 100);
    assert_eq!(comments[0]["id"], "id505");
    assert!(has_note(&response, "truncated (up to 5 pages)"));
    assert_eq!(fixtures.requests().len(), 6);
}

#[test]
fn archive_search_keeps_post_when_top_comment_request_fails() {
    let state = State::new();
    let fixtures = Fixtures::serve(vec![route(
        "/api/posts/search?subreddit=rust&query=farrey&limit=100&sort=desc",
        "archive/post-ids.json",
    )]);
    let response = success(run(
        &state,
        &fixtures,
        &["search", "farrey", "-s", "rust", "--comments", "1"],
    ));
    assert_eq!(response["data"][0]["top_comments"], json!([]));
    assert!(has_note(
        &response,
        "Post 1y1v3iw: could not fetch top comments"
    ));
    assert_eq!(fixtures.requests().len(), 2);
}

#[test]
fn archive_timeline_fetches_all_ties_at_an_upstream_page_boundary() {
    let state = State::new();
    let path = "/api/posts/search?subreddit=rust&limit=100&sort=desc";
    let row = |id: &str, second| json!({"id": id, "created_utc": second});
    let mut first: Vec<_> = (21..=118)
        .rev()
        .map(|second| row(&format!("id{second}"), second))
        .collect();
    first.extend([row("bb", 20), row("aa", 20)]);
    let ties = [row("bb", 20), row("aa", 20), row("cc", 20), row("dd", 19)];
    let fixtures = Fixtures::serve(vec![
        Route::inline(Method::Get, path, json!({"data": first}).to_string(), 200),
        Route::inline(
            Method::Get,
            &format!("{path}&before=21"),
            json!({"data": ties}).to_string(),
            200,
        ),
    ]);
    let response = success(run(&state, &fixtures, &["sub", "rust", "--limit", "101"]));
    let posts = response["data"].as_array().unwrap();
    assert_eq!(posts.len(), 101);
    assert_eq!(posts[98]["id"], "cc");
    assert_eq!(posts[99]["id"], "bb");
    assert_eq!(posts[100]["id"], "aa");
    let after = response["meta"]["after"].as_str().unwrap();
    assert_eq!(after, "a:20:aa");
    let next = success(run(&state, &fixtures, &["sub", "rust", "--after", after]));
    assert_eq!(next["data"].as_array().unwrap().len(), 1);
    assert_eq!(next["data"][0]["id"], "dd");
    assert_eq!(fixtures.requests().len(), 2);
}

#[test]
fn archive_timeline_reports_a_saturated_second_instead_of_skipping_unknown_ids() {
    let state = State::new();
    let rows: Vec<_> = (0..100)
        .map(|id| json!({"id": format!("id{id}"), "created_utc": 20}))
        .collect();
    let fixtures = Fixtures::serve(vec![Route::inline(
        Method::Get,
        "/api/posts/search?subreddit=rust&limit=100&sort=desc",
        json!({"data": rows}).to_string(),
        200,
    )]);
    let error = failure(run(&state, &fixtures, &["sub", "rust"]), 7, "E_UPSTREAM");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("full page at second 20")
    );
    assert_eq!(fixtures.requests().len(), 1);
}
