#!/usr/bin/env node

const { spawn } = require("node:child_process");
const { getRelease } = require("../lib/install.cjs");

try {
  const { binaryPath } = getRelease();
  const child = spawn(binaryPath, process.argv.slice(2), { stdio: "inherit" });

  child.once("error", (error) => {
    console.error(`reddit: ${error.message}. Reinstall with npm lifecycle scripts enabled.`);
    process.exitCode = 1;
  });
  child.once("exit", (code, signal) => {
    if (signal) {
      process.kill(process.pid, signal);
    } else {
      process.exitCode = code;
    }
  });
} catch (error) {
  console.error(`reddit: ${error.message}`);
  process.exitCode = 1;
}
