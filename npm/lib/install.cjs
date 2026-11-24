const { createHash } = require("node:crypto");
const { createWriteStream } = require("node:fs");
const { mkdir, mkdtemp, rename, rm } = require("node:fs/promises");
const path = require("node:path");
const { pipeline } = require("node:stream/promises");
const { version } = require("../package.json");

const targets = {
  "linux-x64": "x86_64-unknown-linux-gnu",
  "linux-arm64": "aarch64-unknown-linux-gnu",
  "darwin-x64": "x86_64-apple-darwin",
  "darwin-arm64": "aarch64-apple-darwin",
  "win32-x64": "x86_64-pc-windows-msvc",
};

function getRelease() {
  const platform = `${process.platform}-${process.arch}`;
  const target = targets[platform];
  if (!target) {
    throw new Error(`unsupported platform ${platform}`);
  }

  const windows = process.platform === "win32";
  const tag = `v${version}`;
  return {
    binaryPath: path.join(__dirname, "..", "vendor", windows ? "reddit.exe" : "reddit"),
    asset: `reddit-${tag}-${target}${windows ? ".exe" : ""}`,
    checksums: `reddit-${tag}-checksums.txt`,
    baseUrl: `https://github.com/Microck/reddit-cli/releases/download/${tag}`,
  };
}

async function download(baseUrl, asset) {
  const response = await fetch(`${baseUrl}/${asset}`, {
    signal: AbortSignal.timeout(60_000),
  });
  if (!response.ok || !response.body) {
    throw new Error(`could not download ${asset} (HTTP ${response.status})`);
  }
  return response;
}

async function install() {
  const { binaryPath, asset, checksums, baseUrl } = getRelease();
  const manifest = await (await download(baseUrl, checksums)).text();
  const expectedHash = manifest.split(/\r?\n/).find((line) => line.slice(66) === asset)?.slice(0, 64);
  if (!expectedHash || !/^[a-f0-9]{64}$/.test(expectedHash)) {
    throw new Error(`missing SHA-256 checksum for ${asset}`);
  }

  const vendorDir = path.dirname(binaryPath);
  await mkdir(vendorDir, { recursive: true });
  const stagingDir = await mkdtemp(path.join(vendorDir, ".install-"));
  const stagedBinary = path.join(stagingDir, path.basename(binaryPath));

  // Stage beside the final path so the launcher can only see a verified binary.
  try {
    const response = await download(baseUrl, asset);
    const hash = createHash("sha256");
    async function* hashChunks(chunks) {
      for await (const chunk of chunks) {
        hash.update(chunk);
        yield chunk;
      }
    }
    await pipeline(response.body, hashChunks, createWriteStream(stagedBinary, { mode: 0o755 }));
    if (hash.digest("hex") !== expectedHash) {
      throw new Error(`SHA-256 checksum mismatch for ${asset}`);
    }
    await rename(stagedBinary, binaryPath);
  } finally {
    await rm(stagingDir, { recursive: true });
  }
}

if (require.main === module) {
  install().catch((error) => {
    console.error(`reddit: ${error.message}`);
    process.exitCode = 1;
  });
}

module.exports = { getRelease };
