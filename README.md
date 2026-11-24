<p align="center">
  <img src=".github/assets/reddit-cli-logo.png" width="720" alt="reddit cli logo">
</p>

<p align="center">
  <a href="https://github.com/Microck/reddit-cli/releases"><picture><source media="(prefers-color-scheme: dark)" srcset="https://shieldcn.dev/github/release/Microck/reddit-cli.svg?variant=secondary&amp;size=xs&amp;logo=false&amp;mode=dark"><img src="https://shieldcn.dev/github/release/Microck/reddit-cli.svg?variant=secondary&amp;size=xs&amp;logo=false&amp;mode=light" alt="release"></picture></a>
  <a href="https://www.npmjs.com/package/@microck/reddit-cli"><picture><source media="(prefers-color-scheme: dark)" srcset="https://shieldcn.dev/npm/dt/@microck/reddit-cli.svg?variant=secondary&amp;size=xs&amp;logo=false&amp;mode=dark"><img src="https://shieldcn.dev/npm/dt/@microck/reddit-cli.svg?variant=secondary&amp;size=xs&amp;logo=false&amp;mode=light" alt="npm total downloads"></picture></a>
  <a href="https://github.com/Microck/reddit-cli/actions/workflows/ci.yml"><picture><source media="(prefers-color-scheme: dark)" srcset="https://shieldcn.dev/github/ci/Microck/reddit-cli.svg?workflow=ci.yml&amp;branch=main&amp;variant=secondary&amp;size=xs&amp;logo=false&amp;mode=dark"><img src="https://shieldcn.dev/github/ci/Microck/reddit-cli.svg?workflow=ci.yml&amp;branch=main&amp;variant=secondary&amp;size=xs&amp;logo=false&amp;mode=light" alt="ci status"></picture></a>
  <a href="LICENSE"><picture><source media="(prefers-color-scheme: dark)" srcset="https://shieldcn.dev/github/license/Microck/reddit-cli.svg?variant=secondary&amp;size=xs&amp;logo=false&amp;mode=dark"><img src="https://shieldcn.dev/github/license/Microck/reddit-cli.svg?variant=secondary&amp;size=xs&amp;logo=false&amp;mode=light" alt="license"></picture></a>
</p>

---

`reddit` is a read-only terminal cli for researching reddit. browse subreddits, search across several communities at once, read full comment threads with the hidden replies expanded, and look up users, through the live reddit api or the arctic shift archive when you have no api access. everything it fetches lands in a local store you can search offline. output is json when piped, so scripts, `jq` and agents read the same thing. `--format md` is for reading.

[documentation](https://reddit.micr.dev/docs) | [npm](https://www.npmjs.com/package/@microck/reddit-cli) | [github](https://github.com/Microck/reddit-cli)

## why

if you research things on reddit, from a terminal or through an agent, this cli gives you one read-only command surface for it. it never posts, votes or writes.

- search one subreddit or several at once, merged and deduplicated, with top comments attached
- read whole threads, including the replies reddit hides behind "load more"
- fall back to the arctic shift archive when you have no api access, with date, author and comment-text filters
- keep everything you fetched in a local sqlite store and search it offline
- get stable json with a `{data, meta}` envelope and typed error codes

## quickstart

### using a package manager

```bash
npm install -g @microck/reddit-cli
cargo install reddit-research --locked   # the crate name; reddit-cli is taken on crates.io
```

if npm asks for approval to run lifecycle scripts:

```bash
npm install -g --allow-scripts=@microck/reddit-cli @microck/reddit-cli
```

### nix / nixos

```bash
nix run github:Microck/reddit-cli -- --source archive sub rust
nix profile install github:Microck/reddit-cli
```

### github releases

download the archive for your os and cpu from [releases](https://github.com/Microck/reddit-cli/releases), check it against the sha-256 manifest, and put `reddit` (or `reddit.exe`) on your `PATH`. builds cover linux (glibc) and macos on x64 and arm64, plus windows on x64.

homebrew, scoop and aur templates live in [packaging/](packaging/).

### first run

no credentials needed for the archive:

```bash
reddit --source archive sub rust --limit 5
```

## data sources

| source | use it for |
| --- | --- |
| `live` | the reddit data api with app-only oauth. live rankings (hot, top, rising), reddit's own search, full threads. |
| `archive` | arctic shift, a free third-party archive. no credentials. comment search, date and author filters. |
| `auto` | the default. `live` when credentials resolve, `archive` otherwise. |

set it with `--source`, `REDDIT_CLI_SOURCE`, or `source` in `config.toml`. the source is picked once per command and never switches silently; when one fails, the error hint names the other.

reddit is closing its public data api. new access ended on 2026-10-31, unregistered apps lose access from 2027-01-12, and all public access ends in march 2027. after that, `--source archive` keeps working for as long as arctic shift does. archive scores and comment counts are captured at posting time and refreshed once about 36 hours later, so newer items show `null`. the [support matrix](docs/spec.md#41-backend-support-matrix) lists which flags each source supports.

## auth

live access uses app-only oauth with your reddit app's client id and secret. the archive needs nothing.

```bash
reddit auth login --client-id YOUR_CLIENT_ID   # prompts for the secret without echo
reddit auth status
```

for scripts, pass `--client-secret-stdin` and pipe the secret in. you can also set `REDDIT_CLIENT_ID`, `REDDIT_CLIENT_SECRET` and `REDDIT_USER_AGENT`, the same variables reddit-mcp-buddy reads; the environment wins over `config.toml` field by field. `reddit auth logout` removes saved credentials.

## command surface

| command | what it does |
| --- | --- |
| `reddit sub NAME` | list a subreddit's posts. defaults to `hot` on live and `new` on the archive. |
| `reddit about NAME` | show subreddit details. |
| `reddit search QUERY` | search posts, comments, subreddits or users with `--type`. repeat `--sub` to search several subreddits; add `--comments K` to attach top comments. archive post and comment search needs `--sub` or `--author`. |
| `reddit thread URL_OR_ID` | read a post and its comments. `--comment ID` focuses one comment, `-o FILE` saves it. on live, `--full` loads hidden replies up to `--max-requests`. |
| `reddit user NAME` | show a user's profile and recent activity. `--posts` or `--comments` for one kind. |
| `reddit cache search QUERY` | full-text search over everything stored locally. |
| `reddit cache stats\|prune\|clear` | inspect or clean the local store. |
| `reddit auth status\|login\|logout` | manage live credentials. |
| `reddit skills list\|get NAME` | print the embedded agent skill. |
| `reddit completions SHELL` | print a shell completion script. |

run `reddit COMMAND --help` for every flag. listings go up to 1000 items; pass `meta.after` back as `--after` to page. dates accept `YYYY-MM-DD`, rfc3339, epoch seconds, or relative values like `30d`.

## output

| format | output |
| --- | --- |
| `auto` | `pretty` in a terminal, `json` when piped. |
| `json` | one `{data, meta}` object on stdout. |
| `jsonl` | one item per line, no envelope. threads print the post, then comments in tree order with `depth`. |
| `md` | compact markdown for reading or handing to a model. |
| `pretty` | human terminal output; its layout can change. |

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

read `meta.notes` for source caveats and partial results. errors go to stderr; with `json` or `jsonl` they are an `{"error": {...}}` object with a stable code, a retry hint and a distinct exit code. the full contract is in [docs/spec.md](docs/spec.md#7-output-contract).

## local store

fetched posts, comments and http responses go into a local sqlite database. entries older than 30 days are pruned automatically; change it with `retention` under `[store]` in `config.toml`. when a fetch shows an item was deleted or removed, its stored text is replaced with the marker and never comes back.

- `--offline` reads only from the store, ignoring cache expiry
- `--refresh` skips cache reads but still saves
- `--no-cache` skips the store entirely
- `reddit cache prune --older-than 7d` and `reddit cache clear` clean it up

`REDDIT_CLI_CONFIG_DIR` and `REDDIT_CLI_DATA_DIR` override the default directories.

## agent usage

`reddit skills get reddit-cli` prints a short guide for agents: the research loop, how to read the output, and the gotchas. [llms.txt](llms.txt) indexes every command, flag, field and error code.

in scripts, use `--format json`, check the exit code, then read `data`. `--quiet` silences notes on stderr.

## shell completion

```bash
reddit completions bash > reddit.bash
reddit completions zsh > _reddit
reddit completions fish > reddit.fish
```

## examples

```bash
# search three subreddits, rank by score, attach two top comments to each hit
reddit search "async runtime" -s rust -s golang -s programming --rank score --comments 2

# read a whole thread, hidden replies included, as markdown
reddit thread https://www.reddit.com/r/rust/comments/1abc23/ --full --format md

# archived comments from the last 30 days, one per line
reddit --source archive --format jsonl search "borrow checker" --type comments -s rust --since 30d

# search what you already fetched, without network
reddit cache search '"borrow checker"' --sub rust --kind post
```

## building from source

install rust with rustup so cargo picks up the version in `rust-toolchain.toml`.

```bash
git clone https://github.com/Microck/reddit-cli.git
cd reddit-cli
cargo install --path . --locked
```

## license

[mit](LICENSE)

## disclaimer

reddit-cli is an independent project and is not affiliated with reddit.
