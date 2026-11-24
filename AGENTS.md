# AGENTS.md

Rules for any agent working in this repo.

## Sources of truth
- `docs/spec.md` defines all observable behavior (commands, flags, JSON shapes,
  error codes, store layout). Read it before changing code. If the spec is wrong,
  say so in your report; do not silently diverge.
- `docs/run-record.md` is the lead's log. Do not edit it.

## Code rules
- Rust 2024, toolchain `1.98.0` (`rust-toolchain.toml`). Blocking reqwest, no tokio.
- Exact dependency pins (`=x.y.z`) with a short comment on why each exists.
- Read-only product: never add endpoints that post, vote, or modify Reddit.
- Validate subreddit/user/ID input before it reaches a URL (spec 3.1). This is a
  security boundary because requests carry a bearer token.
- Never print, log, or commit secrets or tokens.
- No compatibility shims, fallbacks for old local state, or migrations. The
  store fails fast on schema mismatch.
- Keep files under ~800 lines; split by responsibility past that.
- Comment the why: design choices, constraints, non-obvious ordering.
- Do not commit, push, or create branches. The lead integrates.

## Tests
- No mocks or mocking frameworks. Use the `tiny_http` loopback fixture server in
  `tests/support/` with real (trimmed) response bodies in `tests/fixtures/`.
- Unit tests next to pure functions. Focused tests, not smoke-test spam.
- Isolate state with `REDDIT_CLI_CONFIG_DIR` / `REDDIT_CLI_DATA_DIR` tempdirs.
  Never touch the real `~/.config/reddit-cli` in tests.

## Building and testing (Raft)
This host is small (4 CPU, 5 GiB). Run `cargo build`, `cargo clippy`,
`cargo test` on the shared Raft box, not locally. Local use is limited to
`cargo fmt` and reading code.

Box: `paris:rf-f696bb75472943ee` (aarch64 Debian, Rust 1.98.0 via rustup).

```sh
H=paris:rf-f696bb75472943ee
# copy tracked + untracked-unignored files into the box (incremental)
raft sync "$H" /home/ubuntu/.t3/projects/reddit-cli /workspace/reddit-cli
# run the gate (cargo serializes concurrent builds on the shared target dir)
raft exec "$H" -- sh -c '. $HOME/.cargo/env && cd /workspace/reddit-cli && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test'
```

- `raft exec` takes literal args; wrap shell syntax in `sh -c`.
- `raft sync` never deletes guest files. If you delete or rename a source file,
  also remove it in the guest: `raft exec "$H" -- rm /workspace/reddit-cli/<path>`.
- After `cargo` adds deps on the box, copy `Cargo.lock` back:
  `raft download "$H" /workspace/reddit-cli/Cargo.lock /home/ubuntu/.t3/projects/reddit-cli/Cargo.lock`.
- Do not stop, destroy, or create Raft boxes. The lead owns the box lifecycle.
- Live Reddit calls are allowed only for read-only verification, sparingly.
