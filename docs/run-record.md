# reddit-cli run record

Durable record for the multi-agent build. Read after compaction/restart before repeating work.

## Approved scope (user, 2026-11-24)
- Rust CLI, read/research focused (no posting).
- Data: OAuth app-only + anonymous (public .json / RSS) mode.
- Agents-first output: JSON + compact Markdown, skills/ + llms.txt like gitquarry; human table on TTY.
- v1 extras: thread export (full tree incl. morechildren), historical archive (Arctic Shift), multi-sub research, local cache/offline (SQLite FTS).
- Lead: Claude Opus 5.5 (spec, review, integration). Builders: Codex `gpt-6.2-sol` via T3 `delegate_task`.
- Heavy compiles/test suites: Raft workspaces (default location `paris`). Local only for focused lightweight checks.

## Spending
- Parallel deep research: one `pro-fast` run (trun_ce7468b2ceeb4dafacf1c336543ea231). Further paid research: not authorized beyond small follow-ups.
- No new subscriptions/credits.

## Evidence so far
- OAuth client_credentials with creds from ~/.reddit-mcp-buddy/auth.json: token OK, scope `*`, expires_in 86400. oauth.reddit.com returns x-ratelimit-used/remaining(999.0)/reset headers.
- Anonymous from this host (OCI datacenter IP): www/api .json -> 403 HTML block; .rss -> 429; old.reddit .rss -> login redirect. Anonymous mode is best-effort (residential IPs).
- Arctic Shift: /api/posts/search?subreddit=... -> 200 JSON. /api/comments/search `body=` requires author|subreddit|link_id|parent_id.

- Deep research report: scratchpad research/reddit-landscape.md (Data API Wiki 2026-05-11: 100 QPM per client ID over 10 min; unauthenticated traffic is blocked; deleted content must be purged; 48h retention strongly recommended).
- BLOCKER FOUND: Reddit is closing the public Data API (r/redditdev 1wubcvf, 2026-09-30; Reddit Help changelog): no new access requests since 2026-10-31; access removal for unregistered apps from 2027-01-12; all public API access ends March 2027. Apps must move to Devvit (runs inside Reddit; no external API key).
- Arctic Shift: data fresh to 2026-11-24; owner "not sure yet what will happen" re shutdown (issue #76, 2026-10-09). New comment IDs are 13-char base36 (fits spec regex).
- Raft box paris:rf-f696bb75472943ee (persistent, ttl 3d, Rust 1.98.0 via rustup, aarch64).
- docs/spec.md and AGENTS.md drafted. No builders started.

## Decisions after blocker (user, 2026-11-24)
- Backend-agnostic: `--source live|archive|auto` over Reddit OAuth and Arctic Shift. No runtime fallback between backends.
- Anonymous/RSS dropped.
- Store retention default 30d, configurable.
- Arctic Shift keyword search sometimes returns 422 "Timeout. Maybe slow down a bit"; succeeds on retry. Spec: retry once after 2s.
- Fixtures captured by lead into tests/fixtures/ (no tokens).

## Build phases
- P1 foundation (one gpt-6.2-sol builder): everything except src/live/{mod,parse,more}.rs, src/archive/*, packaging/docs. Includes command layer generic over Backend, tested with an in-memory fake backend.
- P2 parallel: live backend | archive backend | packaging+docs.
- P3 lead review + integration + live verification; independent review delegate.

## Active jobs / blockers
- (see below)
- P1 foundation: delegated to codex gpt-6.2-sol (effort high), shared checkout. Thread: "reddit-cli P1 foundation".
- P2c packaging+docs: delegated to codex gpt-6.2-sol (effort medium), shared checkout, disjoint files. npm name `@microck/reddit-cli` (unscoped `reddit-cli` is taken). Thread: "reddit-cli packaging + docs".
- P2a live / P2b archive: start after P1 reports its API.
- P1 DONE (gate green on Raft: fmt, clippy, 15 tests). P2c packaging DONE (README, llms.txt, CI/release, npm @microck/reddit-cli, nix, packaging templates). Both captured in jj change twlxkpru "wip: P1 foundation, packaging, docs" (not a git commit; nothing pushed).
- P2a live + P2b archive: running in jj workspaces /home/ubuntu/.t3/projects/reddit-cli-{live,archive} (children of twlxkpru), Raft dirs /workspace/rc-{live,archive}, targets /workspace/target-{live,archive}.
- Lead edits go in a new jj change on top of twlxkpru in the default workspace.
- Lead: wrote skills/reddit-cli/SKILL.md, Cargo [package] metadata. Naming conflict: `reddit-cli` is taken on BOTH crates.io and npm -> crates.io publish step in release.yml will fail until the user picks a crate name (open question).
- Integration: merged lead+live+archive (jj change ysoxlzpo). Gate 42 tests green.
- LIVE E2E FOUND: Reddit edge 403s rustls clients (HTML block page); same token via OpenSSL = 200. Lead switched reqwest to native-tls-vendored and reclassified live 403 (HTML -> E_UPSTREAM, JSON reason -> E_NOT_FOUND).
- Archive E2E: all commands worked live. Lead fixes: archive name searches ranked by subscribers/karma, notes scoped per command, notes on stderr only for pretty/md. These broke 4 archive tests (expected; fix builder to update).
- Independent review (Claude agent): 13 findings, security boundary clean. Spec updated with decisions (403/3xx mapping, cursor a:<ts>:<id>, sticky tombstones + http_cache purge, --full local prune, archive thread complete=false on prune, --comments K<=100 + 5 page cap + per-post failure note, archive sub default new, offline source=store, loopback-only test overrides, dir 0700 tighten, cache clear try_lock).
- P3 review fixes DONE (codex gpt-6.2-sol): all 13 findings + help text; 58 tests green on Raft.
- Live E2E (user authorized creds via Raft env, tmpfs, deleted after): sub/paging 150/cache/offline, about, search global + 3-sub fan-out with --comments, subs/users search, user, thread (more_count 1662), --full (990 comments, 6 requests, 2617 unresolved, complete=false), -o md, cache search, 302 -> E_NOT_FOUND, 403 gold_only -> E_NOT_FOUND. All correct.
- Archive E2E: all commands correct (earlier run).
- Lead fix: markdown no longer repeats the permalink as the body of text-less self posts.
- Cleanup: jj workspaces live/archive forgotten and deleted; Raft builder dirs removed. Raft box paris:rf-f696bb75472943ee kept (persistent, TTL 3d) for follow-up work.
- Open: crate name (reddit-cli taken on crates.io + npm); GitHub repo not created; nothing committed to git (jj changes only).
- 2026-11-24 user: "decide the name yourself" -> renamed crate/npm/repo/skill/config dir to `reddit-research` (free on crates.io + npm, unscoped). Binary stays `reddit`; env prefix stays REDDIT_CLI_*. UA now `<os>:reddit-research:<ver> (+https://github.com/Microck/reddit-research)`. 58 tests green after rename.
- Created public GitHub repo Microck/reddit-research (empty; nothing pushed; no remote added yet).
- Reddit app registration via browse MCP BLOCKED: developers.reddit.com shows "Your Reddit account (MicrockYT) is banned or suspended, so you cannot use Devvit." Did not appeal.
- 2026-11-24: user said repo must be reddit-cli -> GitHub repo renamed to Microck/reddit-cli; project identity (README, config dir, UA, skill) back to reddit-cli; registry package names stay reddit-research. Squashed into one commit eeddb9f on main (Microck <contact@micr.dev>) and pushed to origin. gitleaks: 2 false positives (comment IDs in fixture).
- 2026-11-24: README hero rewritten to match other CLIs (centered logo .github/assets/reddit-cli-logo.png from user's image, black shields badges, lowercase pitch, links row). Removed template assets/icon.svg. Docs site (Fumadocs, modeled on vinted-cli/docs-site) delegated to codex gpt-6.2-sol; target https://reddit.micr.dev via GitHub Pages. Deploy (enable Pages + DNS) NOT yet authorized.
- 2026-11-24: Snoo cropped out of logo (wordmark only), icon = 'r' glyph; docs-site built (Fumadocs static export, 32 pages, reference generated from --help via make docs); next 16.4.6 + audit fix -> 0 vulns; spec: per-source thread sort default, jsonl has no meta. Committed + pushed (user authorized commit/push).
- 2026-11-24: user authorized docs deploy. Enabled GitHub Pages (Actions source), added Netlify DNS CNAME reddit.micr.dev -> microck.github.io (micr.dev DNS is Netlify/NS1, not Cloudflare), set Pages cname, reran deploy: success, http 200. HTTPS cert pending; background job enforces HTTPS when approved. Installed reddit (built in ubuntu:22.04 container on Raft for glibc 2.35) to ~/.local/bin/reddit; reddit auth login with buddy client id/secret -> ~/.config/reddit-cli/config.toml (0600). Live smoke test OK. Answered: no MCP server in v1.
- 2026-11-24: user: reddit-research only for cargo. npm -> @microck/reddit-cli (unscoped taken), nix/brew/scoop/aur -> reddit-cli. README rewritten lowercase with shieldcn badges (kagi-cli/canonfig style). Repo about/homepage/topics set.
