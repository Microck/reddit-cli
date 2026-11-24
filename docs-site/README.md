# docs-site

the reddit-cli docs use fumadocs on next.js. the pages workflow publishes the
static export at `https://reddit.micr.dev`. the build writes the search index
to `/api/search` and the docs index to `/llms.txt`.

## build

use node.js 22, npm, and python 3. from the repository root, run:

```sh
make docs
cd docs-site
npm ci
npm run typecheck
npm run build
npm run verify
```

`npm ci` runs `fumadocs-mdx` to generate content types. `npm run dev` opens the
development site on port 3000. `npm start` serves the export on port 3000.

## command reference

`make docs` builds the `reddit` binary and calls `scripts/gen-docs-reference.py`
from the repository root. the script reads the root `--help` and follows its
command tree, running `--help` for each command and subcommand. it writes the
reference pages and their sidebar order under `content/docs/reference/`.
edit the help in `src/cli.rs` and regenerate the pages when a command changes.
keep extra notes in other docs or in the handwritten reference index.
