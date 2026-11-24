---
name: reddit-cli
description: Read and research Reddit (subreddits, search, full comment threads, users, historical archive) with the read-only `reddit` CLI. Use when a task needs what people on Reddit said.
allowed-tools: Bash(reddit:*)
---

# reddit

Read-only. Two backends behind one command set:

- `live`: Reddit's API (needs app credentials; `reddit auth status` shows whether they resolve). Current scores, hot/top sorts, full threads.
- `archive`: Arctic Shift. No credentials. Historical search by date, author and comment text. Scores are null until the archive refreshes them (~36 h after posting).

`--source auto` (default) picks `live` when credentials exist, else `archive`. Nothing falls back silently: when a command fails with `E_AUTH` or `E_UNSUPPORTED`, read the `hint` and rerun with the other `--source`.

## Research loop

1. **Find threads.** `reddit search "QUERY" --sub A --sub B --limit 25 --format json`. Repeat `--sub` to fan out across communities; results are merged and deduped. Add `--comments 3` to attach top comments to each hit. For older or dated material use `--source archive --since 2025-01-01 --until 2025-06-30` (archive search needs `--sub` or `--author`).
2. **Read threads.** `reddit thread URL_OR_ID --format md` for reading. Add `--full` (live) when `meta.more_count` > 0 and the hidden replies matter; check `data.complete` and `data.unresolved` afterwards.
3. **Cite.** Every post and comment carries an absolute `permalink`. Quote from `body`/`selftext` and link the permalink.
4. **Revisit offline.** Everything fetched is stored locally; `reddit cache search 'term1 OR term2' --sub NAME` searches it (SQLite FTS5 syntax) without network.

## Output

`--format json` gives `{"data": ..., "meta": {"source", "cached", "fetched_at", "after", "notes"}}`. Read `meta.notes`: they flag partial results, failed subreddits in a fan-out, and archive score caveats. `--format jsonl` streams one item per line; `md` is compact for reading. Pass `meta.after` back as `--after` to page (single `--sub` only).

Errors go to stderr as `{"error": {"code", "message", "retryable", "hint", ...}}` with distinct exit codes. `E_RATE_LIMITED` carries `retry_after` seconds; `E_UPSTREAM` with `retryable: true` from the archive usually succeeds when narrowed (`--since/--until`, `--sub`, `--author`).

## Gotchas

- Share links (`/r/x/s/CODE`) are rejected; ask for the canonical `/comments/ID` URL.
- Live search has no comment search and no date range; that is what `--source archive` is for.
- Reddit's listing cap is 1000 items; `--limit` above that is rejected.
- Reddit is closing its public API (unregistered apps from 2027-01-12, all access by March 2027). When live returns `E_AUTH`, switch to `--source archive`.
