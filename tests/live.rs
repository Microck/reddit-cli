#[allow(dead_code)]
mod support;

use serde_json::{Value, json};
use std::process::Output;
use support::{Fixtures, Route, State};
use tiny_http::Method;

const SMALL: &str = "1y4ddu8";
const BIG: &str = "1xzvb5z";

struct LiveTest {
    server: Fixtures,
    state: State,
}

impl LiveTest {
    fn new(routes: impl IntoIterator<Item = Route>) -> Self {
        Self {
            server: Fixtures::serve(
                std::iter::once(Route::new(Method::Post, "/token", "live/token.json", 200))
                    .chain(routes)
                    .collect(),
            ),
            state: State::new(),
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        self.state
            .command()
            .env("REDDIT_CLIENT_ID", "fixture-client")
            .env("REDDIT_CLIENT_SECRET", "fixture-secret")
            .env("REDDIT_CLI_OAUTH_BASE", &self.server.base)
            .env(
                "REDDIT_CLI_TOKEN_URL",
                format!("{}/token", self.server.base),
            )
            .args(["--source", "live"])
            .args(args)
            .output()
            .unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "{}\nRequests: {:?}",
            String::from_utf8_lossy(&output.stderr),
            self.targets(),
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn targets(&self) -> Vec<String> {
        self.server
            .requests()
            .iter()
            .map(|request| request.target.clone())
            .collect()
    }
}

fn get(target: &str, fixture: &'static str) -> Route {
    Route::new(Method::Get, target, fixture, 200)
}

fn stdout(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn subreddit_listings_page_and_about_uses_the_profile_endpoint() {
    let live = LiveTest::new([
        get("/r/rust/hot?limit=13&raw_json=1", "live/sub-rust-hot.json"),
        get(
            "/r/rust/hot?limit=10&after=t3_1y4brll&raw_json=1",
            "live/sub-rust-new.json",
        ),
        get("/r/rust/about?raw_json=1", "live/about-rust.json"),
        get(
            "/r/AskReddit/top?t=week&limit=1&raw_json=1",
            "live/sub-askreddit-top.json",
        ),
    ]);
    let listing = live.json(&["sub", "r/rust", "--limit", "13"]);
    assert_eq!(listing["data"].as_array().unwrap().len(), 13);
    assert_eq!(listing["data"][0]["id"], "1y3l0sy");
    let image = &listing["data"][3];
    assert_eq!(image["id"], "1y4euiy");
    assert_eq!(image["media"]["type"], "image");
    assert_eq!(
        image["media"]["urls"][0],
        "https://i.redd.it/1rh2tnnhnk3i1.png"
    );
    assert_eq!(listing["meta"]["after"], "t3_1y4b623");
    {
        let requests = live.server.requests();
        assert_eq!(requests[0].method, Method::Post);
        assert!(requests[1].headers.iter().any(|header| {
            header.field.equiv("Authorization") && header.value.as_str() == "Bearer FAKE-TOKEN"
        }));
    }

    let cached = live.json(&["sub", "rust", "--limit", "13", "--offline"]);
    assert_eq!(cached["meta"]["cached"], true);
    assert_eq!(cached["meta"]["source"], "store");
    assert_eq!(live.targets().len(), 3);

    let about = live.json(&["about", "rust"]);
    assert_eq!(about["data"]["name"], "rust");
    assert_eq!(about["data"]["active_users"], Value::Null);
    assert_eq!(about["data"]["url"], "https://www.reddit.com/r/rust/");
    let top = live.json(&[
        "sub",
        "AskReddit",
        "--sort",
        "top",
        "--time",
        "week",
        "--limit",
        "1",
    ]);
    assert_eq!(top["data"][0]["id"], BIG);
}

#[test]
fn post_search_uses_global_and_scoped_endpoints_and_interleaves_subreddits() {
    const RUST: &str =
        "/r/rust/search?q=async&restrict_sr=true&limit=3&sort=relevance&t=all&raw_json=1";
    let live = LiveTest::new([
        get(
            "/search?q=ratatui&limit=3&sort=relevance&t=all&raw_json=1",
            "live/search-global.json",
        ),
        get(RUST, "live/search-rust.json"),
    ]);
    let global = live.json(&["search", "ratatui", "--limit", "3"]);
    assert_eq!(global["data"][0]["id"], "1y3j39t");
    assert_eq!(global["data"][0]["media"]["type"], "video");

    let scoped = live.json(&["search", "async", "--sub", "rust", "--limit", "3"]);
    assert_eq!(scoped["data"][0]["id"], "1r3nrju");
    assert_eq!(scoped["meta"]["after"], "t3_1u4k55x");

    let live = LiveTest::new([
        get(RUST, "live/search-rust.json"),
        get(
            "/r/rust+madeinrust/search?q=async&restrict_sr=true&limit=3&sort=relevance&t=all&raw_json=1",
            "live/sub-rust-hot.json",
        ),
    ]);
    let multi = live.json(&[
        "search",
        "async",
        "-s",
        "rust",
        "-s",
        "rust+madeinrust",
        "--limit",
        "3",
    ]);
    assert_eq!(multi["data"][0]["id"], "1r3nrju");
    assert_eq!(multi["data"][1]["id"], "1y3l0sy");
    assert_eq!(multi["meta"]["after"], Value::Null);
    assert_eq!(live.targets().len(), 3);
}

#[test]
fn search_attaches_only_requested_root_comments() {
    let live = LiveTest::new([
        get(
            "/search?q=habits&limit=1&sort=relevance&t=all&raw_json=1",
            "live/sub-askreddit-top.json",
        ),
        get(
            "/comments/1xzvb5z?sort=top&depth=1&limit=2&raw_json=1",
            "live/thread-big.json",
        ),
    ]);
    let result = live.json(&["search", "habits", "--limit", "1", "--comments", "2"]);
    let top = result["data"][0]["top_comments"].as_array().unwrap();
    assert_eq!(top.len(), 2);
    assert_eq!(top[0]["id"], "19svnsjsgl0v8");
    for comment in top {
        assert_eq!(comment["depth"], 0);
        assert_eq!(comment["parent_id"], "t3_1xzvb5z");
        assert_eq!(comment["replies"], serde_json::json!([]));
    }
}

#[test]
fn searches_subreddit_and_user_names_and_reads_user_overview() {
    let live = LiveTest::new([
        get(
            "/subreddits/search?q=rust&limit=2&raw_json=1",
            "live/search-subs.json",
        ),
        get(
            "/users/search?q=spez&limit=2&raw_json=1",
            "live/search-users.json",
        ),
        get("/user/spez/about?raw_json=1", "live/user-spez-about.json"),
        get(
            "/user/spez/overview?limit=4&sort=new&t=all&raw_json=1",
            "live/user-spez-overview.json",
        ),
    ]);
    let subs = live.json(&["search", "rust", "--type", "subs", "--limit", "2"]);
    assert_eq!(subs["data"][0]["name"], "rust");
    assert_eq!(subs["meta"]["after"], "t5_2xks2");
    let users = live.json(&["search", "spez", "--type", "users", "--limit", "2"]);
    assert_eq!(users["data"][0]["name"], "spez");
    assert_eq!(users["data"][0]["total_karma"], Value::Null);
    assert_eq!(users["meta"]["after"], "t2_4gm3yvpn");

    let profile = live.json(&["user", "u/spez", "--limit", "4"]);
    assert_eq!(profile["data"]["user"]["name"], "spez");
    assert_eq!(profile["data"]["user"]["total_karma"], 941417);
    assert_eq!(profile["data"]["items"].as_array().unwrap().len(), 4);
    assert_eq!(profile["data"]["items"][0]["kind"], "comment");
    assert_eq!(profile["data"]["items"][0]["id"], "19ruo33ofgh8i");
    assert_eq!(profile["meta"]["after"], "t1_19runzmofvco5");
}

#[test]
fn small_thread_is_complete_and_renders_as_markdown_and_jsonl() {
    let live = LiveTest::new([get(
        "/comments/1y4ddu8?limit=200&sort=confidence&raw_json=1",
        "live/thread-small.json",
    )]);
    let result = live.json(&["thread", "https://redd.it/1y4ddu8"]);
    assert_eq!(result["data"]["post"]["id"], SMALL);
    assert_eq!(result["data"]["complete"], true);
    assert_eq!(result["data"]["comments_returned"], 3);
    assert_eq!(result["meta"]["more_count"], 0);
    assert_eq!(result["meta"]["requests"], 1);
    let permalink = result["data"]["comments"][0]["permalink"].as_str().unwrap();
    assert!(permalink.starts_with("https://www.reddit.com/r/rust/comments/"));

    let md = stdout(live.run(&["thread", SMALL, "--format", "md"]));
    assert!(md.starts_with("# Where have you found the Rust type-state pattern"));
    assert!(md.contains("\n---\n\n- **u/"));
    let jsonl = stdout(live.run(&["thread", SMALL, "--format", "jsonl"]));
    let lines: Vec<Value> = jsonl
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[0]["kind"], "post");
    assert_eq!(lines[1]["id"], "19t4dtd6ft2f4");
    assert_eq!(lines[1]["depth"], 0);
    assert_eq!(live.targets().len(), 2);
}

#[test]
fn big_thread_counts_listed_hidden_ids_without_expansion() {
    let live = LiveTest::new([get(
        "/comments/1xzvb5z?limit=40&depth=3&sort=confidence&raw_json=1",
        "live/thread-big.json",
    )]);
    let result = live.json(&["thread", BIG, "--limit", "40", "--depth", "3"]);
    assert_eq!(result["data"]["complete"], false);
    assert_eq!(result["data"]["comments_returned"], 40);
    assert_eq!(result["data"]["unresolved"], serde_json::json!([]));
    assert_eq!(result["meta"]["more_count"], 1548);
    assert_eq!(live.targets().len(), 2);
}

// The capture contains 20 requested comments and their returned descendants.
const FIRST_BATCH: &str = concat!(
    "19sx1boobkiw0,19swas0q5tztx,19svxnv76wlnk,19sw20h7uujdv,19svy8djp68wx,19sw87bagilyd,",
    "19swgeonl4iht,19svsgipre3dw,19sw8lw9t9oyp,19swsf008sm8w,19sw2tsnuc0ip,19sxi7d1gnq1u,",
    "19t2squ3gks6b,19t4dzazto1ph,19sw041sba5ya,19sw47ikcrzc1,19sxdwqsc734y,19sw65h6tkqea,",
    "19swcf74gr63n,19swelm6d24dc,19swuffwigcyq,19swzvxuc3mar,19t12g4n2tdib,19sxxtxi34t5v,",
    "19szix0klmba9,19szxkatdnkbk,19t2zvmtjajdx,19t3o2e3zkuqp,19sv7wuye56si,19svjf982kb6o,",
    "19svnt84spm05,19svnnc9ji6cj,19svnfkcq4dk4,19svloipjgy3n,19svgnft0chi9,19svn730rfs84,",
    "19svowisvrixu,19svvvfi302hw,19sxa28uhilr8,19swd4xzuw145,19svr1ubveqas,19sw8bn7uc3ok,",
    "19sx39qinwpep,19sxzu9myap44,19svxdqfdyen7,19svykjgn35wj,19swutfmb2qw2,19svq3hvfbb1g,",
    "19sxaz8394o0z,19svuluiqmoit,19swdua1m7stg,19t1dto1bw3ad,19svrjruvi9i9,19svs1pt7hd40,",
    "19svxb630l7z7,19svwcg7fmt2b,19svwyv0jysgz,19svy6nmjsc6a,19sw7bhsjkazn,19syjn6h6wtk1,19svqd0g28zhc,",
    "19svwtpy919k1,19sw4v71dsazo,19swvb4cq1kf9,19t2dthkrxs4k,19t2j9hj3nkpw,19t49frr16qar,",
    "19svp1dl6qi05"
);

#[test]
fn full_thread_follows_stubs_sequentially_and_stops_at_request_budget() {
    let live = LiveTest::new([
        get(
            "/comments/1xzvb5z?limit=500&sort=confidence&raw_json=1",
            "live/thread-big.json",
        ),
        get(
            &format!(
                "/api/morechildren?children={FIRST_BATCH}&api_type=json&link_id=t3_1xzvb5z&sort=confidence&raw_json=1"
            ),
            "live/morechildren.json",
        ),
        get(
            "/comments/1xzvb5z?comment=19suuix07xz8i&limit=500&sort=confidence&raw_json=1",
            "live/thread-big.json",
        ),
    ]);
    let result = live.json(&[
        "thread",
        BIG,
        "--full",
        "--max-requests",
        "3",
        "--depth",
        "1",
        "--limit",
        "2",
    ]);
    assert_eq!(result["meta"]["requests"], 3);
    assert_eq!(result["data"]["complete"], false);
    assert_eq!(result["data"]["comments_returned"], 2);
    assert!(has_note(&result, "dropped 73 comments"));
    let unresolved = result["data"]["unresolved"].as_array().unwrap();
    assert!(
        unresolved
            .iter()
            .any(|item| item["reason"] == "request_budget_exhausted")
    );
    assert!(
        unresolved
            .iter()
            .any(|item| item["id"] == "19swuffwigcyq" && item["reason"] == "not_returned")
    );
    assert!(
        unresolved.iter().any(|item| item["id"] == "19suuix07xz8i"
            && item["reason"] == "continuation_still_truncated")
    );
    assert!(!unresolved.iter().any(|item| item["id"] == "19svxnv76wlnk"));
    let first_root = &result["data"]["comments"][0];
    let replies = first_root["replies"].as_array().unwrap();
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["depth"], 1);
    assert_eq!(replies[0]["replies"], json!([]));
    let targets = live.targets();
    assert_eq!(targets.len(), 4);
    assert!(targets[1].starts_with("/comments/1xzvb5z?limit=500"));
    assert!(targets[2].starts_with("/api/morechildren?"));
    assert!(targets[3].starts_with("/comments/1xzvb5z?comment=19suuix07xz8i"));
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
    let failure: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(failure["error"]["code"], code);
    failure["error"].clone()
}

#[test]
fn live_redirects_and_denials_map_to_their_contract_errors() {
    for (status, body, exit, code) in [
        (302, "", 4, "E_NOT_FOUND"),
        (403, r#"{"reason":"private"}"#, 4, "E_NOT_FOUND"),
        (403, "<html>Access denied</html>", 7, "E_UPSTREAM"),
        (403, "{}", 3, "E_AUTH"),
    ] {
        let live =
            LiveTest::new([
                Route::inline(Method::Get, "/r/rust/about?raw_json=1", body, status)
                    .header("Location", "/subreddits/search?raw_json=1"),
            ]);
        let error = failure(live.run(&["about", "rust"]), exit, code);
        assert_eq!(error["http_status"], status);
        assert!(error["hint"].as_str().unwrap().contains("--source archive"));
        assert_eq!(live.targets().len(), 2);
    }
}

#[test]
fn live_401_refreshes_once_and_retries_with_the_new_token() {
    let path = "/r/rust/about?raw_json=1";
    let live = LiveTest::new([
        Route::inline(Method::Get, path, "{}", 401),
        Route::inline(
            Method::Post,
            "/token",
            r#"{"access_token":"REFRESHED-TOKEN","expires_in":3600}"#,
            200,
        ),
        get(path, "live/about-rust.json"),
    ]);
    assert_eq!(live.json(&["about", "rust"])["data"]["name"], "rust");
    assert_eq!(live.targets(), ["/token", path, "/token", path]);
    let requests = live.server.requests();
    for (index, token) in [(1, "Bearer FAKE-TOKEN"), (3, "Bearer REFRESHED-TOKEN")] {
        assert!(requests[index].headers.iter().any(|header| {
            header.field.equiv("Authorization") && header.value.as_str() == token
        }));
    }
}

#[test]
fn full_thread_stops_on_expansion_error_and_returns_remaining_work() {
    let continuation =
        "/comments/1xzvb5z?comment=19suuix07xz8i&limit=500&sort=confidence&raw_json=1";
    let failed = || Route::inline(Method::Get, continuation, "{}", 503).header("Retry-After", "0");
    let live = LiveTest::new([
        get(
            "/comments/1xzvb5z?limit=500&sort=confidence&raw_json=1",
            "live/thread-big.json",
        ),
        get(
            &format!(
                "/api/morechildren?children={FIRST_BATCH}&api_type=json&link_id=t3_1xzvb5z&sort=confidence&raw_json=1"
            ),
            "live/morechildren.json",
        ),
        failed(),
        failed(),
        failed(),
    ]);
    let result = live.json(&["thread", BIG, "--full"]);
    assert_eq!(result["meta"]["requests"], 3);
    assert_eq!(result["data"]["complete"], false);
    assert_eq!(result["data"]["comments_returned"], 75);
    let unresolved = result["data"]["unresolved"].as_array().unwrap();
    assert!(
        unresolved
            .iter()
            .any(|item| item["id"] == "19suuix07xz8i" && item["reason"] == "upstream_error")
    );
    assert!(
        unresolved
            .iter()
            .filter(|item| item["reason"] == "upstream_error")
            .count()
            > 1
    );
    assert!(
        !unresolved
            .iter()
            .any(|item| item["reason"] == "request_budget_exhausted")
    );
    let targets = live.targets();
    assert_eq!(targets.len(), 6);
    assert!(
        targets[3..]
            .iter()
            .all(|target| target.contains("comment=19suuix07xz8i"))
    );
}

#[test]
fn live_search_keeps_post_when_top_comment_request_fails() {
    let live = LiveTest::new([get(
        "/search?q=habits&limit=1&sort=relevance&t=all&raw_json=1",
        "live/sub-askreddit-top.json",
    )]);
    let result = live.json(&["search", "habits", "--limit", "1", "--comments", "2"]);
    assert_eq!(result["data"][0]["top_comments"], json!([]));
    assert!(has_note(
        &result,
        "Post 1xzvb5z: could not fetch top comments"
    ));
    assert_eq!(live.targets().len(), 3);
}

#[test]
fn multi_sub_search_keeps_successful_subreddit_and_notes_failure() {
    let live = LiveTest::new([get(
        "/r/rust/search?q=async&restrict_sr=true&limit=3&sort=relevance&t=all&raw_json=1",
        "live/search-rust.json",
    )]);
    let result = live.json(&[
        "search", "async", "-s", "ruby", "-s", "rust", "--limit", "3",
    ]);
    assert_eq!(result["data"][0]["id"], "1r3nrju");
    assert_eq!(result["meta"]["after"], Value::Null);
    assert!(has_note(&result, "r/ruby:"));
    assert_eq!(live.targets().len(), 3);
}

#[test]
fn live_tombstone_purges_cache_and_survives_newer_archive_text() {
    let old = json!({
        "id": "abc", "name": "t3_abc", "subreddit": "rust",
        "author": "fixture-author", "title": "privateword", "selftext": "privateword",
        "created_utc": 10,
    });
    let mut deleted = old.clone();
    deleted["author"] = json!("[deleted]");
    deleted["selftext"] = json!("[deleted]");
    let listing = |post| {
        json!({"kind":"Listing", "data":{"children":[{"kind":"t3", "data":post}]}}).to_string()
    };
    let archive_path = "/api/posts/search?subreddit=rust&limit=100&sort=desc";
    let mut archived = old.clone();
    archived.as_object_mut().unwrap().remove("name");
    let archive_body = json!({"data":[archived]}).to_string();
    let live = LiveTest::new([
        Route::inline(
            Method::Get,
            "/r/rust/hot?limit=1&raw_json=1",
            listing(old),
            200,
        ),
        Route::inline(
            Method::Get,
            "/r/rust/new?limit=1&raw_json=1",
            listing(deleted),
            200,
        ),
        Route::inline(Method::Get, archive_path, archive_body.clone(), 200),
        Route::inline(Method::Get, archive_path, archive_body, 200),
    ]);
    let local = |args: &[&str]| {
        let output = live.state.run(args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };
    let archive = |flags: &[&str]| {
        live.state
            .command()
            .env(
                "REDDIT_CLI_ARCTIC_BASE",
                format!("{}/api", live.server.base),
            )
            .args(["--source", "archive", "sub", "rust"])
            .args(flags)
            .output()
            .unwrap()
    };

    live.json(&["sub", "rust", "--limit", "1"]);
    assert!(archive(&[]).status.success());
    assert_eq!(
        local(&["cache", "search", "privateword"])["data"][0]["id"],
        "abc"
    );
    live.json(&["sub", "rust", "--limit", "1", "--sort", "new"]);
    failure(
        live.run(&["--offline", "sub", "rust", "--limit", "1"]),
        8,
        "E_OFFLINE_MISS",
    );
    failure(archive(&["--offline"]), 8, "E_OFFLINE_MISS");

    // Make the archive observation strictly newer without depending on wall-clock timing.
    let db = rusqlite::Connection::open(live.state.paths.data_dir.join("store.sqlite")).unwrap();
    db.execute("UPDATE items SET fetched_at = fetched_at - 1", [])
        .unwrap();
    assert!(archive(&[]).status.success());
    let marker = local(&["cache", "search", "deleted", "--kind", "post"]);
    assert_eq!(marker["data"][0]["title"], "[deleted]");
    assert_eq!(marker["data"][0]["selftext"], "[deleted]");
    assert_eq!(
        local(&["cache", "search", "privateword"])["data"],
        json!([])
    );
    failure(archive(&["--offline"]), 8, "E_OFFLINE_MISS");
    assert_eq!(live.targets().len(), 5);
}

#[test]
fn live_full_thread_is_incomplete_when_local_pruning_is_the_only_loss() {
    let live = LiveTest::new([get(
        "/comments/1y4ddu8?limit=500&sort=confidence&raw_json=1",
        "live/thread-small.json",
    )]);
    let result = live.json(&["thread", SMALL, "--full", "--depth", "0", "--limit", "1"]);
    assert_eq!(result["data"]["comments_returned"], 1);
    assert_eq!(result["data"]["complete"], false);
    assert_eq!(result["data"]["unresolved"], json!([]));
    assert!(has_note(&result, "dropped 2 comments"));
    assert_eq!(live.targets().len(), 2);
}
