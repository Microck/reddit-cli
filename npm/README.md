# reddit-research

Install the native Reddit research CLI with npm.

```sh
npm install -g reddit-research
reddit --help
```

The postinstall script downloads and verifies the binary for this package
version. It supports Linux with glibc and macOS on x64 and ARM64, and Windows
on x64. Leave npm lifecycle scripts enabled during installation. The package
requires Node.js 18 or later.

If npm requires approval for lifecycle scripts, install with
`npm install -g --allow-scripts=reddit-research reddit-research`.

[Commands and configuration](https://github.com/Microck/reddit-cli#readme)
