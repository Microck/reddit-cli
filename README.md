<p align="center">
  <img src=".github/assets/reddit-cli-logo.png" width="720" alt="reddit cli logo">
</p>

<p align="center">
  <a href="https://github.com/Microck/reddit-cli/releases"><img src="https://img.shields.io/github/v/release/Microck/reddit-cli?display_name=tag&style=flat-square&label=release&color=000000" alt="release badge"></a>
  <a href="https://www.npmjs.com/package/reddit-research"><img src="https://img.shields.io/npm/dt/reddit-research?style=flat-square&label=downloads&color=000000" alt="npm downloads"></a>
  <a href="https://github.com/Microck/reddit-cli/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/Microck/reddit-cli/ci.yml?branch=main&style=flat-square&label=ci&color=000000" alt="ci badge"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-mit-000000?style=flat-square" alt="license badge"></a>
</p>

---

`reddit` is a read-only terminal cli for researching reddit. browse subreddits, search across several communities at once, read full comment threads with the hidden replies expanded, and look up users, through the live reddit api or the arctic shift archive when you have no api access. everything it fetches lands in a local store you can search offline. output is json when piped, so scripts, `jq` and agents read the same thing. `--format md` is for reading.

[documentation](https://reddit.micr.dev/docs) | [npm](https://www.npmjs.com/package/reddit-research) | [github](https://github.com/Microck/reddit-cli)

## why

Use `reddit` to browse subreddits, search posts and comments, read threads, and
look up users from a terminal or script. The CLI has read-only access to Reddit
and saves fetched posts and comments in a searchable local store.

## quickstart

| Channel | How to install |
| --- | --- |
| GitHub Releases | Download the archive for your OS and CPU. Verify it against the release's SHA-256 manifest, then extract `reddit` or `reddit.exe` to a directory on your `PATH`. [Releases](https://github.com/Microck/reddit-cli/releases) |
| npm | Run `npm install -g reddit-research` with Node.js 18 or later. Postinstall downloads the matching release binary. |
| crates.io | Run `cargo install reddit-research --locked`. |
| Nix | Run `nix profile install github:Microck/reddit-cli`. |
| Homebrew | Once a tap carries the rendered formula, run `brew install OWNER/TAP/reddit-research` with its tap name. [Formula template](packaging/homebrew/reddit-research.rb.template) |
| Scoop | Fill in the release version and checksum in the template, save it as `reddit-research.json`, then run `scoop install .\reddit-research.json`. [Manifest template](packaging/scoop/reddit-research.json.template) |
| Arch Linux | Fill in the release version and source checksum, then run `makepkg -si` in the directory with the `PKGBUILD`. [PKGBUILD](packaging/aur/PKGBUILD) |

GitHub Releases and npm cover Linux with glibc and macOS on x64 and ARM64,
plus Windows on x64.

If npm requires approval for lifecycle scripts, install with
`npm install -g --allow-scripts=reddit-research reddit-research`.

Try a listing from the archive without credentials.

```sh
reddit --source archive sub rust --limit 5
```

## data sources

| Source | Use it for |
| --- | --- |
| `live` | The Reddit Data API with app-only OAuth. Use it for live subreddit rankings and Reddit's search. |
| `archive` | Arctic Shift, a free third-party service that requires no credentials. Use it for comment search and date or author filters. |
| `auto` | Select `live` when credentials resolve and `archive` otherwise. |

Set the source with `--source`, `REDDIT_CLI_SOURCE`, or `source` in
`config.toml`, in that order. The default is `auto`. A command uses the
selected source for its entire run. If a request fails, you can select the other
source with `--source`.

Reddit is closing its public Data API. New access ended on October 31, 2026.
Unregistered apps lose access on January 12, 2027, and all public access ends in
March 2027. The archive uses a separate service. Its scores and comment counts
are snapshots, and fields can be null when Arctic Shift has only the first
capture. See the [backend support matrix](docs/spec.md#41-backend-support-matrix)
for source-specific flags.

## auth

Live access uses app-only OAuth with a Reddit app's client credentials.
Set `REDDIT_CLIENT_ID` and `REDDIT_CLIENT_SECRET` in the environment. You can
also set `REDDIT_USER_AGENT`. The CLI checks the environment before the
`[auth]` section of `config.toml` for each field.

To save and verify credentials, run the command below and enter the secret at
the hidden prompt. For scripted login, pass `--client-secret-stdin` and supply
the secret on standard input.

```sh
reddit auth login --client-id YOUR_CLIENT_ID
reddit auth status
```

Use `reddit auth logout` to sign out. The archive does not use Reddit
credentials.

## command surface

| Command | What it does |
| --- | --- |
| `reddit sub NAME` | Lists subreddit posts. Use `--sort new` with the archive. |
| `reddit about NAME` | Shows subreddit details. |
| `reddit search QUERY` | Searches posts, comments, subreddits, or users via `--type posts\|comments\|subs\|users`. Repeat `--sub` to search several subreddits. Archive post and comment searches require `--sub` or `--author`. |
| `reddit thread URL_OR_ID` | Reads a post and its comment tree. Use `--comment ID` to focus on a comment or `-o FILE` to save the output. On live, `--full` expands hidden replies up to `--max-requests`. |
| `reddit user NAME` | Shows a user overview by default. Use `--posts` or `--comments` for one kind of activity. |
| `reddit cache search FTS_QUERY` | Searches locally stored posts and comments with SQLite FTS5. |
| `reddit cache stats\|prune\|clear` | Inspects or manages the local store. |
| `reddit auth status\|login\|logout` | Inspects or manages live credentials. |
| `reddit skills list\|get NAME` | Lists embedded skills or prints a named skill. |
| `reddit completions SHELL` | Prints a shell completion script. |

Use `reddit COMMAND --help` for flags. Listing limits go up to 1000. Pass
`meta.after` back as `--after` with the same source to resume a listing.
For multi-subreddit searches, omit `--after`. Date filters accept dates,
RFC3339 timestamps, epoch seconds, and relative values such as `30d`.

## output formats and the JSON envelope

| Format | Output |
| --- | --- |
| `auto` | Pretty output in a terminal and JSON when stdout is redirected or piped. |
| `json` | One object with `data` and `meta` on stdout. |
| `jsonl` | One item per line for lists. Threads emit the post first, then comments in tree order with `depth`. Other payloads use the JSON envelope on one line. |
| `md` | Markdown for reading or passing to a model. |
| `pretty` | Human-readable terminal output. Its layout can change. |

Set the format with `--format` or `REDDIT_CLI_FORMAT`. A JSON response for
an empty search can look like this.

```json
{
  "data": [],
  "meta": {
    "source": "archive",
    "cached": false,
    "fetched_at": "2026-11-24T15:00:00Z",
    "after": null,
    "notes": []
  }
}
```

Check `meta.notes` for source caveats and partial multi-subreddit failures.
Errors go to stderr. With `json` or `jsonl`, stderr carries an `error` object
with a stable code, retry information, and a hint when available.
[Output and error contract](docs/spec.md#7-output-contract)

## local store

The CLI saves HTTP responses and fetched posts and comments in a local SQLite
database. The default retention is `30d`. Set `retention` under `[store]` in
`config.toml` to change it. The CLI prunes old entries when it opens the store,
at most once per hour. A fresh fetch that marks an item as deleted or removed
replaces its stored text with the corresponding marker.

Use `--offline` to read from the store without network requests, even after the
normal cache TTL expires. Use `--refresh` to bypass cache reads while still
saving fresh results, or `--no-cache` to skip cache reads and writes.

Run `reddit cache prune --older-than 7d` to remove older entries. Run
`reddit cache clear` to delete the database and cached OAuth token.
`REDDIT_CLI_CONFIG_DIR` and `REDDIT_CLI_DATA_DIR` override the default
platform directories.

## agent usage

Run `reddit skills list` to find the embedded usage guides. Run
`reddit skills get NAME` to print one, or read the [command index](llms.txt)
for flags, output fields, and error codes.

In scripts, request `--format json`, parse stdout, and check the exit code
before reading `data`. Use `--quiet` to suppress progress and notes on stderr.

## shell completion

Generate a script for your shell and install it in the shell's completion
directory.

```sh
reddit completions bash > reddit.bash
reddit completions zsh > _reddit
reddit completions fish > reddit.fish
```

## examples

```sh
# Search archived posts in two subreddits.
reddit --source archive search 'embedded Rust' -s rust -s embedded --since 30d --rank new

# Search archived comments and stream one result per line.
reddit --source archive --format jsonl search 'borrow checker' --type comments -s rust

# Save a thread to Markdown using a post ID.
reddit --source archive thread 1abc23 --sort top -o thread.md

# Search posts already in the local store.
reddit cache search '"borrow checker"' --sub rust --kind post
```

## building from source

Install Rust through rustup so Cargo uses the version in
`rust-toolchain.toml`.

```sh
git clone https://github.com/Microck/reddit-cli.git
cd reddit-cli
cargo install --path . --locked
```

## license

[MIT](LICENSE).

## disclaimer

reddit-cli is an independent project. It is not affiliated with Reddit.
