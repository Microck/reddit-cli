# reddit-cli spec and contract (v1)

This file is the source of truth for observable behavior. Code, tests and docs
follow it. When code and spec disagree, fix the spec first (with a reason), then
the code.

## 1. Product

`reddit` is a read-only Reddit research CLI for agents and humans. It browses
subreddits, searches, reads full comment threads and looks up users through
either the live Reddit API or the Arctic Shift archive, and keeps a local
SQLite store that can be searched offline. It never posts, votes or writes to Reddit.

- Project/repo: `reddit-cli`. Crate and npm package: `reddit-research` (`reddit-cli` is taken on both registries). Binary: `reddit`.
- Rust edition 2024, toolchain pinned in `rust-toolchain.toml` to `1.98.0`.
- Blocking I/O (`reqwest` blocking + native-tls, see 5.1). No async runtime. Use
  `std::thread::scope` for the few places that fan out (multi-sub search,
  `--comments`).
- Exact dependency pins (`=x.y.z`) with a one-line comment per dependency on
  why it exists.

## 2. Backends

Reddit is closing its public Data API (no new access since 2026-10-31,
unregistered apps cut from 2027-01-12, all public access ends March 2027). The
CLI therefore has one command surface over two backends:

- `live`: Reddit Data API via app-only OAuth (`oauth.reddit.com`).
- `archive`: Arctic Shift (`arctic-shift.photon-reddit.com/api`), no auth.

Selection: `--source live|archive|auto` (global), else `REDDIT_CLI_SOURCE`, else
`config.toml` `source`, else `auto`. `auto` = `live` when OAuth credentials
resolve, else `archive`. Selection happens once, before any request. There is
no runtime fallback between backends: a failing backend fails the command with
a hint naming the other one. No anonymous/unauthenticated Reddit access and no
RSS (Reddit's policy blocks unauthenticated traffic; observed 403/429/login
walls).

Every command declares which backends it supports and which flags each backend
honors. An unsupported combination fails before any request with
`E_UNSUPPORTED` naming the flag/backend and the alternative.

## 3. Global flags

| Flag | Values | Default | Meaning |
|---|---|---|---|
| `--format` | `auto`, `json`, `jsonl`, `md`, `pretty` | `auto` | `auto` = `pretty` when stdout is a TTY, else `json`. Env `REDDIT_CLI_FORMAT`. |
| `--source` | `auto`, `live`, `archive` | `auto` | See section 2. Ignored by `cache`, `auth`, `skills`. |
| `--offline` | flag | off | Serve only from the local store, ignoring TTL. Miss = `E_OFFLINE_MISS`. No network. |
| `--refresh` | flag | off | Skip cache reads, still write. |
| `--no-cache` | flag | off | Skip cache reads and writes (item store too). |
| `-q, --quiet` | flag | off | No notes on stderr. Notes go to stderr only for `pretty`/`md`; `json`/`jsonl` carry them in `meta.notes` only. |

## 4. Commands

```
reddit auth status
reddit auth login --client-id ID [--client-secret-stdin] [--user-agent UA]
reddit auth logout

reddit sub NAME [--sort hot|new|top|rising|controversial (default: live hot, archive new)] [--time hour|day|week|month|year|all]
                [--limit N=25] [--after CURSOR]
reddit about NAME
reddit search QUERY [-s|--sub NAME]... [--type posts|comments|subs|users =posts]
                    [--sort relevance|hot|top|new|comments =relevance] [--time ...=all]
                    [--author U] [--since DATE] [--until DATE]
                    [--limit N=25] [--after CURSOR]
                    [--rank interleave|score|new|comments =interleave] [--comments K=0]
reddit thread URL_OR_ID [--comment ID] [--sort confidence|top|new|controversial|old|qa (default: live confidence, archive top)]
                        [--depth N] [--limit N=200] [--full] [--max-requests N=50] [-o FILE]
reddit user NAME [--posts | --comments | --overview (default)] [--sort new|hot|top|controversial =new]
                 [--time ...=all] [--since DATE] [--until DATE] [--limit N=25] [--after CURSOR]

reddit cache search FTS_QUERY [-s|--sub NAME] [--author U] [--kind post|comment] [--limit N=25]
reddit cache stats
reddit cache prune [--older-than DURATION]   # default = configured retention
reddit cache clear

reddit skills list
reddit skills get NAME
reddit completions SHELL
```

`--limit` max 1000. Live listings page transparently at 100 per request;
archive pages at 100 per request. `--after` resumes from a previous
`meta.after`; cursors are opaque and only valid for the backend that issued
them (live: Reddit fullname `t3_x`; archive: `a:<created_utc>:<id>`, where the next page re-includes items at that exact second that sort after `<id>`, so ties at a page boundary are never skipped). Archive subreddit/user name searches return one page and no cursor. A cursor from
the other backend is `E_USAGE`.

`DATE` accepts `YYYY-MM-DD`, RFC3339, epoch seconds, or relative `30d`, `12h`,
`6m` (months), `1y`. Normalize to epoch seconds before sending.

### 4.1 Backend support matrix

| Command / flag | live | archive |
|---|---|---|
| `sub` `--sort hot/rising/controversial/top` | yes | no (`E_UNSUPPORTED`) |
| `sub --sort new` | yes | yes (`/posts/search?subreddit=&sort=desc`) |
| `about` | `/r/S/about` | `/subreddits/search?subreddit=S` (first exact match, else `E_NOT_FOUND`) |
| `search --type posts` | `/search` or `/r/S/search` | `/posts/search?query=` (requires `--sub` or `--author`) |
| `search --type comments` | no | `/comments/search?body=` (requires `--sub` or `--author`) |
| `search --type subs` | `/subreddits/search?q=` | `/subreddits/search?subreddit_prefix=&sort_type=subscribers` (one page) |
| `search --type users` | `/users/search?q=` | `/users/search?author_prefix=&sort_type=total_karma` (one page) |
| `search --author/--since/--until` | no | yes |
| `search --sort` | all values | `new` only (desc) or `relevance` treated as `new`; others `E_UNSUPPORTED` |
| `search --time` | yes | no (use `--since/--until`) |
| `search --comments K` (K ≤ 100) | yes | yes (archive: `/comments/search?link_id=&parent_id=` roots, at most 5 pages per post, sorted by score locally; a note says when the cap truncated) |
| `thread` | `/comments/ID` + morechildren | `/posts/ids?ids=` + `/comments/tree?link_id=t3_ID&limit=9999` |
| `thread --sort/--depth/--limit` | yes | fetch the whole tree (`limit=9999`), then `--sort top/new/old`, `--depth` and `--limit` apply locally; if pruning drops comments, `complete` is false and a note says how many were dropped |
| `thread --full/--max-requests` | yes | ignored (tree is already complete; note added) |
| `user` overview/posts/comments | `/user/U/about` + listing | `/users/search?author=U` + `/posts/search?author=` / `/comments/search?author=` merged by time |
| `user --sort hot/top/controversial`, `--time` | yes | no |
| `user --since/--until` | no | yes |

Archive notes always added to `meta.notes`:
- Scores and comment counts are captured at posting time and refreshed once
  ~36 h later. An item whose `_meta.retrieved_2nd_on` is absent has
  `score`/`num_comments`/`upvote_ratio` = `null` (its stored values are 0/1
  placeholders).
- Archive keyword search is a simple keyword match for posts and Postgres FTS
  (`websearch_to_tsquery` syntax) for comments; very active subs/users may
  time out.

### 4.2 Semantics worth pinning

- **Name validation (security boundary, the bearer token must never be steered
  to another endpoint):** subreddit `^[A-Za-z0-9_]{2,21}$`, a `+`-joined
  multireddit of those (live only), or `u_<user>`; strip a leading `r/` or
  `/r/`. Username `^[A-Za-z0-9_-]{3,20}$`; strip `u/`, `/u/`, `user/`.
  Post/comment IDs are base36 `^[a-z0-9]{2,13}$` after stripping `t3_`/`t1_`,
  lowercased. All query values are URL-encoded by the HTTP client, never
  string-concatenated.
- **URL/ID parsing** (`thread`): accept bare ID, `t3_ID`, any `*.reddit.com`
  host (`www`, `old`, `new`, `np`, `m`), `/r/S/comments/ID[/slug[/CID]]`,
  `/user/U/comments/ID...`, `/comments/ID`, `redd.it/ID`, `/gallery/ID`. Strip
  query, fragment and `.json`. A `/r/S/s/CODE` share link cannot be resolved without anonymous access, so it
  fails with `E_SHARE_LINK` and the hint to open it in a browser and paste the
  canonical URL. A URL with a comment ID behaves as `--comment CID`.
- **search multi-sub (`--sub` repeated):** one request chain per subreddit,
  each with the full `--limit`, concurrently (max 4 threads). Merge, dedupe by
  post ID, then by `crosspost_parent` (keep highest score). Ranking:
  `interleave` = round-robin by each sub's own order; `score`, `new`,
  `comments` = global sort (null scores last). Truncate to `--limit`. A failing
  sub becomes a `meta.notes` entry, not a failure, unless every sub fails.
  `--after` with more than one `--sub` is `E_USAGE`.
- **search `--comments K`:** after ranking, fetch the top K root comments for
  each result (live: `/comments/ID?sort=top&depth=1&limit=K`) and attach as
  `top_comments`. Concurrency max 4. A post whose comment fetch fails gets
  `top_comments: []` and a `meta.notes` entry; the search still succeeds.
- **thread (live):** default fetch `/comments/ID?limit=&depth=&sort=`. Without
  `--full`, `more` stubs are counted into `meta.more_count` (hidden comment IDs)
  and dropped. With `--full`: first request `limit=500`, no depth; then
  `/api/morechildren` with `api_type=json`, `link_id=t3_ID`, `children=` up to
  100 IDs per call, `sort` = `--sort`, strictly one request at a time. A `more`
  stub with empty `children` ("continue this thread") is fetched as
  `/comments/ID?comment=PARENT&limit=500`, once per parent. Never re-request an
  ID Reddit did not return. Stop at `--max-requests` total; leftover IDs go to
  `data.unresolved` with a reason. Rebuild the tree from a flat map and
  recompute `depth`, then apply `--depth` and `--limit` locally (dropped comments
  make `complete` false, with a note). `data.complete` is true only when nothing
  is unresolved or dropped. A failing request during expansion (after the shared
  retries) stops expansion: the batch and the remaining queue go to
  `unresolved` with reason `upstream_error` and the partial tree is returned.
  Same flat-map rebuild is used for the archive tree.
- **thread `-o FILE`:** writes the rendered output (`md` if the file ends in
  `.md`, `json` for `.json`, `jsonl` for `.jsonl`, else `--format`) and prints
  `{"data":{"path":..., "bytes":...}}` (or a pretty line).
- **Arctic Shift errors:** bodies are `{"data": null, "error": "..."}`.
  An error text containing `Timeout` (observed as HTTP 422 "Timeout. Maybe slow
  down a bit", often a cold query that succeeds on the next try) is retried once
  after 2 s; if it fails again → `E_UPSTREAM`, retryable, with the hint to
  narrow `--since/--until` or add `--sub/--author`. Other 400/422 → `E_USAGE`
  carrying the `error` text. 5xx → `E_UPSTREAM`.
- **cache search:** SQLite FTS5 over stored posts and comments (title, body,
  author, subreddit), ranked by `bm25`. FTS syntax error → `E_USAGE` with
  SQLite's message.

## 5. Live backend: auth and limits

### 5.1 OAuth (app-only)
- Grant `client_credentials` at `POST https://www.reddit.com/api/v1/access_token`,
  HTTP Basic `client_id:client_secret`, form body. Validate `access_token` and
  `expires_in > 0`. A body containing `error` is a failure even on 200.
- API host `https://oauth.reddit.com`; always add `raw_json=1`.
- Token cached in the data dir (`token.json`, mode 0600) with `expires_at`;
  expired 60 s early. A 401 triggers one token refresh and one retry. Live 403
  is classified by body: an HTML page (Reddit's edge blocked the request) →
  `E_UPSTREAM`; JSON with a `reason` (`private`, `banned`, `quarantined`,
  `gold_only`) → `E_NOT_FOUND`; anything else → `E_AUTH` with the hint that the
  app may have lost Data API access and `--source archive` still works.
  Redirects are never followed: a live 3xx (Reddit redirects missing
  subreddits to search) → `E_NOT_FOUND`.
- TLS: `reqwest` with `native-tls` (vendored OpenSSL on Linux). Reddit's edge
  serves an HTML 403 to rustls clients (verified 2026-11-24).
- Credential resolution per field, first hit wins: env `REDDIT_CLIENT_ID`,
  `REDDIT_CLIENT_SECRET`, `REDDIT_USER_AGENT` (same names reddit-mcp-buddy
  uses), then `config.toml` `[auth]` `client_id`, `client_secret`,
  `user_agent`. Empty values or unresolved placeholders (`${X}`, `$X`) are unset.
- `auth login` writes `config.toml` (file 0600, dir 0700), secret from stdin
  (`--client-secret-stdin`, or a no-echo prompt on a TTY), then fetches a token
  to prove the credentials and reports `{ok, scope, expires_in}`.
  `auth status` reports each field's origin (`env`/`config`/`unset`), resolved
  `source`, token validity, last rate-limit state. Never print secrets.
- User-Agent default `<os>:reddit-cli:<version> (+https://github.com/Microck/reddit-cli)`. Truthful;
  never impersonate a browser.

### 5.2 Rate limiting (shared across processes)
Agents call the CLI many times in a row, so limit state lives in the store.
- After each response record `(host, remaining, reset_at)` from
  `x-ratelimit-remaining`/`x-ratelimit-reset` (live) or
  `X-RateLimit-Remaining`/`X-RateLimit-Reset` (archive), when present.
- Before a request: if `remaining < 1` and `reset_at` is in the future, sleep
  when the wait is ≤ 15 s, else `E_RATE_LIMITED` with `retry_after`.
- On 429/503: wait `Retry-After` (seconds or HTTP date), else the reset header,
  else 500 ms × 2^n ±20 % jitter. Max 2 retries, only when the wait is ≤ 15 s.
- Timeouts (15 s per request) and connection resets: same backoff, max 2.
- Archive is a free service: max 4 concurrent requests per process.

### 5.3 Test overrides (hidden from help)
`REDDIT_CLI_OAUTH_BASE`, `REDDIT_CLI_TOKEN_URL`, `REDDIT_CLI_ARCTIC_BASE`
(accepted only when they point at a loopback host, `127.0.0.1`/`::1`/`localhost`,
so an inherited environment cannot redirect credentials); paths
`REDDIT_CLI_CONFIG_DIR`, `REDDIT_CLI_DATA_DIR`.

## 6. Local store

- Paths: config `$REDDIT_CLI_CONFIG_DIR` else XDG config `reddit-cli/`
  (`config.toml`). Data `$REDDIT_CLI_DATA_DIR` else XDG data `reddit-cli/`
  (`store.sqlite`, `token.json`). Use the `directories` crate.
- SQLite via `rusqlite` with `bundled` (FTS5 included). WAL mode,
  `busy_timeout` 5 s so parallel CLI processes don't fail.
- Tables:
  - `http_cache(key TEXT PK, body BLOB, fetched_at INT, ttl INT)`; key =
    `source|method|url-with-sorted-query` (never includes the token).
  - `rate_limit(host TEXT PK, remaining REAL, reset_at INT, window TEXT)`.
  - `items(fullname TEXT PK, kind TEXT, id TEXT, subreddit TEXT, author TEXT,
    title TEXT, body TEXT, link_id TEXT, parent_id TEXT, score INT,
    created_utc INT, permalink TEXT, json TEXT, source TEXT, fetched_at INT)`
    plus an external-content FTS5 table over `title, body, author, subreddit`
    kept in sync by triggers.
  - `meta(key PK, value)` with `schema_version`. On mismatch: fail with
    `E_STORE_SCHEMA` and the hint `reddit cache clear` (hard cut, no migration).
- TTLs (HTTP cache): live listings and search 5 min, live threads 10 min,
  about/user 15 min, archive responses 60 min.
- Retention: `config.toml` `[store] retention` (duration, default `30d`).
  Items and cache rows older than retention are deleted automatically when the
  store is opened, at most once per hour (tracked in `meta`).
- Every post/comment returned by any command is upserted into `items` (unless
  `--no-cache`). If a fetch shows an item as deleted/removed, overwrite the
  stored title/body with the marker so the store never keeps content its author
  deleted. Tombstones are sticky: a later fetch from any source (the archive
  often still has the text) never restores content over a stored marker. When a
  tombstone is first written, delete `http_cache` rows whose body contains that
  item's fullname so `--offline` cannot serve the old text.
- Config and data directories are created 0700, and an existing directory is
  tightened to 0700.
- `cache clear` takes the store lock without waiting; if another process holds
  it → `E_CONFIG` "store is in use".
- `cache prune` deletes `http_cache` and `items` older than the duration
  (default = retention); `cache clear` deletes the store file and token.

## 7. Output contract

### 6.1 JSON (`--format json`)
Every success is one object on stdout:

```json
{"data": <payload>, "meta": {"source": "live|archive|store", "cached": false,
  "fetched_at": "2026-11-24T15:00:00Z", "after": null, "notes": []}}
```

`meta.source` is `store` for `--offline` and `cache` commands.
`meta` may carry command-specific keys (`more_count`, `requests`, `rate_limit`).
Field names are snake_case and stable; absent values are `null`, never omitted.
Timestamps: `created_utc` (integer seconds) and `created` (RFC3339 UTC).

Payloads:
- `sub`, `search --type posts`: `Post[]`
- `search --type comments`: `Comment[]` (flat, `replies: []`)
- `search --type subs`, `about`: `Subreddit[]` / `Subreddit`
- `search --type users`: `UserSummary[]`
- `thread`: `Thread`
- `user` (all modes): `{ "user": User, "items": (Post|Comment)[] }`
  with each item tagged `"kind": "post"|"comment"`
- `cache search`: tagged `(Post|Comment)[]`

Core shapes (all fields always present):

```
Post { kind:"post", id, fullname, subreddit, author, title, selftext, url, domain, permalink,
       created_utc, created, score|null, upvote_ratio|null, num_comments|null,
       over_18, spoiler, stickied, locked, is_self, flair|null, crosspost_parent|null,
       media: { type: "none"|"image"|"gallery"|"video"|"link", urls: [] },
       state: "ok"|"deleted"|"removed", top_comments?: Comment[] }
Comment { kind:"comment", id, fullname, link_id, parent_id, subreddit, author, body,
          created_utc, created, score|null, depth, is_submitter, stickied,
          distinguished|null, edited: bool, state, permalink, replies: Comment[] }
Thread { post: Post, comments: Comment[], complete: bool,
         unresolved: [{ id, reason }], comments_returned: int }
Subreddit { name, title, description, subscribers|null, active_users|null,
            created_utc, created, over_18, url }
User { name, created_utc, created, link_karma, comment_karma, total_karma,
       is_suspended, is_employee }
UserSummary { name, created_utc, created, total_karma|null }
```

`permalink` is always absolute (`https://www.reddit.com/...`). `state` is
`deleted` when author is `[deleted]` and body/selftext is `[deleted]`,
`removed` when body/selftext is `[removed]` or `removed_by_category` is set.
`media.urls` uses `url_overridden_by_dest`, `media_metadata` (gallery, in
`gallery_data` order, entity-decoded), `secure_media.reddit_video.fallback_url`,
`preview.images[0].source.url`.

### 6.2 JSONL
List payloads: one item per line, no envelope. Non-list payloads: same as JSON
on one line. `thread`: post line then comments flattened in pre-order, each
with `depth`.
`jsonl` has no envelope, so it carries no `meta`; use `json` when `meta.notes`,
`meta.after` or rate-limit state matter.

### 6.3 Markdown (`md`)
Compact, for reading and LLM context. Threads:

```
# <title>
r/<sub> · u/<author> · <score> pts · <num_comments> comments · <YYYY-MM-DD> · <permalink>

<selftext or url>

---

- **u/a** · 12 pts · 2026-01-01
  body line 1
  body line 2
  - **u/b** (OP) · 3 pts · 2026-01-02
    reply body
```

Listings: numbered list, one post per item: `**title** · r/sub · u/author ·
score pts · N comments · date` then the permalink on the next line, then up to
280 chars of selftext. Unknown scores render as `? pts`.

### 6.4 Pretty
Human terminal output, color only when stdout is a TTY and `NO_COLOR` is unset.
Not a stable contract.

### 6.5 Errors
stderr. When the resolved format is `json` or `jsonl`, one JSON object:

```json
{"error": {"code": "E_RATE_LIMITED", "message": "...", "retryable": true,
  "http_status": 429, "retry_after": 42, "hint": "...", "suggested_commands": ["reddit auth status"]}}
```

Otherwise `error[E_CODE]: message` plus `hint:` line.

| Code | Exit | When |
|---|---|---|
| `E_USAGE` | 2 | bad args (clap errors too) |
| `E_AUTH` | 3 | missing/invalid credentials, token endpoint rejects |
| `E_CONFIG` | 3 | unreadable config, bad permissions |
| `E_NOT_FOUND` | 4 | 404, live 3xx, banned/private sub (403 with `reason`), missing post/user |
| `E_RATE_LIMITED` | 5 | limit hit and wait > 15 s |
| `E_UNSUPPORTED` | 2 | flag/command not supported by the selected backend |
| `E_NETWORK` | 6 | DNS/connect/timeout after retries |
| `E_UPSTREAM` | 7 | 5xx, unparseable body, HTML block page, archive timeout |
| `E_OFFLINE_MISS` | 8 | `--offline` and not in store |
| `E_SHARE_LINK` | 2 | share link given (cannot be resolved) |
| `E_STORE_SCHEMA` | 9 | store schema mismatch |

## 8. Architecture and file ownership

```
src/main.rs            entry, calls app::run
src/cli.rs             clap derive: Cli, Command, *Args (whole surface declared)
src/app.rs             dispatch, format resolution, error rendering, exit codes
src/error.rs           AppError { code, message, hint, http_status, retry_after, ... }
src/config.rs          paths, config.toml, credential resolution
src/model.rs           Post, Comment, Thread, Subreddit, User, UserSummary, Item, Meta, Envelope
src/output/            mod.rs (render dispatch), markdown.rs, pretty.rs
src/http.rs            shared blocking client, retries, rate-limit gate, cache read/write
src/store.rs           SQLite: http_cache, rate_limit, items + FTS, schema check
src/backend.rs         Backend selection + the Backend trait the commands call
src/ids.rs             name validation, URL/ID parsing, DATE parsing
src/tree.rs            flat-map comment tree rebuild, depth recompute, local sort/prune
src/live/mod.rs        LiveBackend (implements Backend) over oauth.reddit.com
src/live/auth.rs       token fetch/cache
src/live/parse.rs      Reddit JSON -> model (things, listings, media, state)
src/live/more.rs       morechildren / continue-thread expansion
src/archive/mod.rs     ArchiveBackend (implements Backend) over Arctic Shift
src/archive/parse.rs   Arctic Shift JSON -> model
src/commands/*.rs      one file per command group
src/agent.rs           embedded skills (include_str!)
tests/support/mod.rs   tiny_http loopback fixture server + temp dirs
tests/fixtures/        recorded JSON bodies (trimmed, real shapes)
```

Backend trait (in `src/backend.rs`). Commands only talk to this; each backend
returns `E_UNSUPPORTED` for what its column in 4.1 says "no". Validation of
names/IDs happens in `ids.rs` before the call.

```rust
pub struct Page<T> { pub items: Vec<T>, pub after: Option<String>, pub notes: Vec<String> }

pub trait Backend: Sync {
    fn kind(&self) -> SourceKind; // Live | Archive
    fn subreddit_posts(&self, q: &SubQuery) -> Result<Page<Post>, AppError>;
    fn subreddit_about(&self, name: &SubName) -> Result<Subreddit, AppError>;
    fn search_posts(&self, q: &SearchQuery) -> Result<Page<Post>, AppError>; // one sub or none
    fn search_comments(&self, q: &SearchQuery) -> Result<Page<Comment>, AppError>;
    fn search_subreddits(&self, q: &SearchQuery) -> Result<Page<Subreddit>, AppError>;
    fn search_users(&self, q: &SearchQuery) -> Result<Page<UserSummary>, AppError>;
    fn top_comments(&self, post: &PostId, k: usize) -> Result<Page<Comment>, AppError>; // notes carry truncation
    fn thread(&self, q: &ThreadQuery) -> Result<(Thread, ThreadMeta), AppError>;
    fn user_about(&self, name: &UserName) -> Result<User, AppError>;
    fn user_items(&self, q: &UserQuery) -> Result<Page<Item>, AppError>;
}
```

Multi-sub fan-out, ranking, `--comments` attachment, store upserts and output
rendering live in the command layer so both backends share them.

## 9. Testing rules

- No mocks or mocking frameworks. HTTP tests run the compiled binary against a
  real `tiny_http` loopback server serving files from `tests/fixtures/`, with
  `REDDIT_CLI_*_BASE` and isolated `REDDIT_CLI_CONFIG_DIR`/`DATA_DIR` tempdirs.
- Unit tests live next to pure functions (ID parsing, DATE parsing, tree
  building, ranking, media extraction).
- Fixtures come from real responses, trimmed. Never commit tokens.
- Gate: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test --locked`. Run heavy builds/tests on Raft (see AGENTS.md).
