mod support;

use reddit_research::{
    app::resolve_format,
    backend::SourceKind,
    cli::Format,
    error::ErrorCode,
    http::{CachePolicy, Endpoints, HttpClient, LISTING_TTL},
    model::{Post, now},
    store::Store,
};
use serde_json::{Value, json};
use std::{
    fs,
    process::{Output, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use support::{Fixtures, Route, State, with_stdin};
use tiny_http::Method;

fn success(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn failure(output: Output, exit: i32, code: &str) -> Value {
    assert_eq!(
        output.status.code(),
        Some(exit),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], code);
    error
}

#[test]
fn login_status_and_logout_use_private_state_and_hide_secrets() {
    let state = State::new();
    let fixtures = Fixtures::serve(vec![Route::new(
        Method::Post,
        "/token",
        "live/token.json",
        200,
    )]);
    let mut login = state.command();
    login
        .env("REDDIT_CLI_TOKEN_URL", format!("{}/token", fixtures.base))
        .args([
            "auth",
            "login",
            "--client-id",
            "fixture-client",
            "--client-secret-stdin",
            "--user-agent",
            "fixture-agent",
        ]);
    let output = with_stdin(&mut login, b"fixture-secret\n");
    for stream in [&output.stdout, &output.stderr] {
        let text = String::from_utf8_lossy(stream);
        assert!(!text.contains("fixture-secret"));
        assert!(!text.contains("FAKE-TOKEN"));
    }
    assert_eq!(
        success(output)["data"],
        json!({"ok":true, "scope":"*", "expires_in":86400})
    );
    {
        let requests = fixtures.requests();
        let request = &requests[0];
        assert_eq!(request.method, Method::Post);
        assert_eq!(request.target, "/token");
        assert_eq!(request.body, "grant_type=client_credentials");
        assert!(
            request
                .headers
                .iter()
                .any(|header| header.field.equiv("Authorization")
                    && header.value.as_str().starts_with("Basic "))
        );
        assert!(
            request
                .headers
                .iter()
                .any(|header| header.field.equiv("User-Agent")
                    && header.value.as_str() == "fixture-agent")
        );
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [state.paths.config(), state.paths.token()] {
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert_eq!(
            fs::metadata(&state.paths.config_dir)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }

    let status = success(state.run(&["auth", "status", "--source", "archive"]));
    assert_eq!(status["data"]["source"], "live");
    assert_eq!(status["data"]["credentials"]["client_secret"], "config");
    assert_eq!(status["data"]["token_valid"], true);
    success(state.run(&["auth", "logout"]));
    assert!(!state.paths.token().exists());
    let status = success(state.run(&["auth", "status"]));
    assert_eq!(status["data"]["source"], "archive");
    assert_eq!(status["data"]["token_valid"], false);
}

#[test]
fn error_envelope_and_output_format_follow_cli_and_environment() {
    let state = State::new();
    assert_eq!(resolve_format(None, true), Format::Pretty);
    assert_eq!(resolve_format(Some(Format::Auto), false), Format::Json);
    assert_eq!(resolve_format(Some(Format::Md), true), Format::Md);
    failure(
        state.run(&["sub", "rust", "--limit", "1001", "--format", "jsonl"]),
        2,
        "E_USAGE",
    );
    failure(
        state.run(&["thread", "/r/rust/s/example"]),
        2,
        "E_SHARE_LINK",
    );
    let unsupported = failure(
        state.run(&["--source", "archive", "sub", "rust", "--sort", "hot"]),
        2,
        "E_UNSUPPORTED",
    );
    assert!(
        unsupported["error"]["message"]
            .as_str()
            .unwrap()
            .contains("--sort hot")
    );
    assert_eq!(unsupported["error"]["hint"], "Try --source live.");
    let output = state
        .command()
        .env("REDDIT_CLI_FORMAT", "md")
        .args(["cache", "stats"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("```json"));
    let output = state
        .command()
        .env("REDDIT_CLI_FORMAT", "md")
        .args(["cache", "stats", "--format=json"])
        .output()
        .unwrap();
    assert_eq!(success(output)["meta"]["source"], "store");
}

#[test]
fn oauth_error_body_on_success_status_is_rejected() {
    let state = State::new();
    let fixtures = Fixtures::serve(vec![Route::new(
        Method::Post,
        "/token",
        "live/token-error.json",
        200,
    )]);
    let output = with_stdin(
        state
            .command()
            .env("REDDIT_CLI_TOKEN_URL", format!("{}/token", fixtures.base))
            .args([
                "auth",
                "login",
                "--client-id",
                "fixture-client",
                "--client-secret-stdin",
            ]),
        b"fixture-secret\n",
    );
    let error = failure(output, 3, "E_AUTH");
    assert_eq!(error["error"]["http_status"], 200);
    assert!(!error.to_string().contains("fixture-secret"));
    assert!(!state.paths.token().exists());
}

#[test]
fn cache_search_stats_prune_and_clear_work_through_the_binary() {
    let state = State::new();
    let store = Store::open(&state.paths, 30 * 86_400).unwrap();
    let post = Post {
        id: "ab".into(),
        fullname: "t3_ab".into(),
        title: "The searchable term".into(),
        subreddit: "rust".into(),
        author: "fixture-user".into(),
        ..Post::default()
    };
    store
        .upsert(&json!(post), SourceKind::Archive, now() - 2 * 86_400)
        .unwrap();
    store
        .write_cache("example", b"{}", now() - 2 * 86_400, 60)
        .unwrap();
    drop(store);
    let stats = success(state.run(&["cache", "stats"]));
    assert_eq!(stats["meta"]["source"], "store");
    assert_eq!(
        stats["data"],
        json!({ "http_cache": 1, "items": 1, "posts": 1, "comments": 0 })
    );
    let search = success(state.run(&[
        "cache",
        "search",
        "searchable",
        "--sub",
        "RUST",
        "--kind",
        "post",
    ]));
    assert_eq!(search["data"][0]["fullname"], "t3_ab");
    assert_eq!(search["meta"]["source"], "store");
    failure(state.run(&["cache", "search", "("]), 2, "E_USAGE");
    let prune = success(state.run(&["cache", "prune", "--older-than", "1d"]));
    assert_eq!(prune["data"], json!({ "http_cache": 1, "items": 1 }));
    assert_eq!(success(state.run(&["cache", "stats"]))["data"]["items"], 0);
    fs::write(state.paths.token(), "{}").unwrap();
    let connection = rusqlite::Connection::open(state.paths.data_dir.join("store.sqlite")).unwrap();
    connection
        .execute(
            "UPDATE meta SET value = '999' WHERE key = 'schema_version'",
            [],
        )
        .unwrap();
    drop(connection);
    failure(state.run(&["cache", "stats"]), 9, "E_STORE_SCHEMA");
    success(state.run(&["cache", "clear"]));
    assert!(!state.paths.data_dir.join("store.sqlite").exists());
    assert!(!state.paths.token().exists());
}

#[test]
fn shared_http_retries_429_and_serves_offline_cache_without_network() {
    let fixtures = Fixtures::serve(vec![
        Route::new(
            Method::Get,
            "/api/query?sub=rust&query=two+words",
            "archive/error-timeout.json",
            429,
        )
        .header("Retry-After", "0"),
        Route::new(
            Method::Get,
            "/api/query?sub=rust&query=two+words",
            "archive/posts-sub-rust.json",
            200,
        ),
    ]);
    let store = Arc::new(Store::in_memory().unwrap());
    let client = |policy| {
        HttpClient::new(
            Arc::clone(&store),
            policy,
            Endpoints::new(&fixtures.base, &format!("{}/api", fixtures.base)).unwrap(),
            "fixture-agent",
        )
        .unwrap()
    };
    let get = |client: &HttpClient, path| {
        client.get(
            SourceKind::Archive,
            path,
            &[("sub", "rust"), ("query", "two words")],
            LISTING_TTL,
            None,
        )
    };
    let online = get(&client(CachePolicy::default()), "/query").unwrap();
    assert!(!online.cached);
    assert!(online.body["data"].is_array());
    assert_eq!(fixtures.requests().len(), 2);
    let offline = client(CachePolicy {
        offline: true,
        refresh: true,
        no_cache: false,
    });
    let cached = get(&offline, "/query").unwrap();
    assert_eq!(cached.body, online.body);
    assert!(cached.cached);
    assert!(offline.observation().cached);
    assert_eq!(
        get(&offline, "/absent").unwrap_err().code,
        ErrorCode::EOfflineMiss
    );
    assert_eq!(fixtures.requests().len(), 2);
}

#[test]
fn rate_limit_headers_persist_and_gate_short_and_long_waits() {
    for reset in ["3", "60"] {
        let state = State::new();
        let fixtures = Fixtures::serve(vec![
            Route::new(
                Method::Get,
                "/api/subreddits/search?subreddit=rust",
                "archive/subreddit-rust.json",
                200,
            )
            .header("X-RateLimit-Remaining", "0")
            .header("X-RateLimit-Reset", reset),
            Route::inline(
                Method::Get,
                "/api/subreddits/search?subreddit=rust",
                r#"{"data":[{"display_name":"rust"}]}"#,
                200,
            ),
        ]);
        let run = |refresh: bool| {
            let mut command = state.command();
            command
                .env("REDDIT_CLI_ARCTIC_BASE", format!("{}/api", fixtures.base))
                .args(["--source", "archive", "about", "rust"]);
            if refresh {
                command.arg("--refresh");
            }
            command.output().unwrap()
        };
        let first = success(run(false));
        let rate = &first["meta"]["rate_limit"];
        assert_eq!(rate["remaining"], 0.0);
        assert_eq!(rate["host"], "127.0.0.1");
        let reset_at = rate["reset_at"].as_i64().unwrap();
        let store = Store::open(&state.paths, 30 * 86_400).unwrap();
        assert_eq!(
            store.rate_limit("127.0.0.1").unwrap().unwrap().reset_at,
            reset_at
        );
        drop(store);

        if reset == "3" {
            let wait = reset_at - now();
            assert!((1..=3).contains(&wait));
            let start = Instant::now();
            assert_eq!(success(run(true))["data"]["name"], "rust");
            assert!(start.elapsed() >= Duration::from_secs((wait - 1).max(1) as u64));
            assert_eq!(fixtures.requests().len(), 2);
        } else {
            let error = failure(run(true), 5, "E_RATE_LIMITED");
            assert!(error["error"]["retry_after"].as_i64().unwrap() > 15);
            assert_eq!(fixtures.requests().len(), 1);
        }
    }
}

#[test]
fn retention_prunes_only_after_an_hour_between_store_opens() {
    let state = State::new();
    let retention = 30 * 86_400;
    let store = Store::open(&state.paths, retention).unwrap();
    let post = Post {
        id: "ab".into(),
        fullname: "t3_ab".into(),
        ..Post::default()
    };
    let stale = now() - retention - 60;
    store.upsert(&json!(post), SourceKind::Live, stale).unwrap();
    store.write_cache("stale", b"{}", stale, 60).unwrap();
    drop(store);

    let before_hour = success(state.run(&["cache", "stats"]));
    assert_eq!(before_hour["data"]["items"], 1);
    assert_eq!(before_hour["data"]["http_cache"], 1);
    let db = rusqlite::Connection::open(state.paths.data_dir.join("store.sqlite")).unwrap();
    db.execute(
        "UPDATE meta SET value = ?1 WHERE key = 'last_prune'",
        [now() - 3600],
    )
    .unwrap();
    let after_hour = success(state.run(&["cache", "stats"]));
    assert_eq!(after_hour["data"]["items"], 0);
    assert_eq!(after_hour["data"]["http_cache"], 0);
}

#[cfg(unix)]
#[test]
fn existing_config_and_data_directories_are_tightened_to_0700() {
    use std::os::unix::fs::PermissionsExt;

    let state = State::new();
    for dir in [&state.paths.config_dir, &state.paths.data_dir] {
        fs::create_dir(dir).unwrap();
        fs::set_permissions(dir, fs::Permissions::from_mode(0o755)).unwrap();
    }
    success(state.run(&["cache", "stats"]));
    for dir in [&state.paths.config_dir, &state.paths.data_dir] {
        assert_eq!(
            fs::metadata(dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
}

#[test]
fn endpoint_overrides_reject_non_loopback_hosts_before_requests() {
    let state = State::new();
    let fixtures = Fixtures::serve(vec![]);
    for key in [
        "REDDIT_CLI_OAUTH_BASE",
        "REDDIT_CLI_TOKEN_URL",
        "REDDIT_CLI_ARCTIC_BASE",
    ] {
        for host in ["https://example.invalid", "https://127.0.0.2"] {
            let output = state
                .command()
                .env("REDDIT_CLIENT_ID", "fixture-client")
                .env("REDDIT_CLIENT_SECRET", "fixture-secret")
                .env("REDDIT_CLI_OAUTH_BASE", &fixtures.base)
                .env("REDDIT_CLI_TOKEN_URL", format!("{}/token", fixtures.base))
                .env(key, host)
                .args(["--source", "live", "about", "rust"])
                .output()
                .unwrap();
            let error = failure(output, 3, "E_CONFIG");
            assert!(error["error"]["message"].as_str().unwrap().contains(key));
        }
    }
    assert!(fixtures.requests().is_empty());
}

#[test]
fn cache_clear_fails_without_waiting_for_a_held_store_lock() {
    let state = State::new();
    let store = Store::open(&state.paths, 30 * 86_400).unwrap();
    let mut child = state
        .command()
        .args(["cache", "clear"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let completed = loop {
        if child.try_wait().unwrap().is_some() {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        thread::sleep(Duration::from_millis(10));
    };
    assert!(state.paths.data_dir.join("store.sqlite").exists());
    drop(store);
    let output = child.wait_with_output().unwrap();
    assert!(completed, "cache clear waited for the store lock");
    let error = failure(output, 3, "E_CONFIG");
    assert_eq!(error["error"]["message"], "store is in use");
    let cleared = success(state.run(&["cache", "clear"]));
    assert_eq!(cleared["meta"]["source"], "store");
    assert!(!state.paths.data_dir.join("store.sqlite").exists());
}
