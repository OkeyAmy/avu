#!/usr/bin/env node
const { spawnSync } = require("node:child_process");
const { existsSync } = require("node:fs");
const { join } = require("node:path");

const executable = process.platform === "win32" ? "avu.exe" : "avu";
const binary = join(__dirname, "native", executable);

if (!existsSync(binary)) {
  console.error("Avu native binary is missing.");
  console.error("Run: node npm/install.js");
  console.error("If npm scripts were disabled, reinstall with scripts enabled.");
  process.exit(1);
}

const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });

if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}

process.exit(result.status ?? 0);
