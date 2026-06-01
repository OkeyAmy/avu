#!/usr/bin/env node
const { createReadStream, createWriteStream, chmodSync, existsSync, mkdirSync, readFileSync, rmSync } = require("node:fs");
const { get } = require("node:https");
const { join } = require("node:path");
const { spawnSync } = require("node:child_process");
const { createHash } = require("node:crypto");

const repo = "https://github.com/OkeyAmy/avu";
const version = process.env.AVU_INSTALL_VERSION || require("../../package.json").version;

function target() {
  const arch = process.arch;
  const platform = process.platform;
  const cpu = arch === "x64" ? "x86_64" : arch === "arm64" ? "aarch64" : arch;
  if (platform === "linux") return { asset: `avu-${cpu}-unknown-linux-musl.tar.gz`, exe: "avu" };
  if (platform === "darwin") return { asset: `avu-${cpu}-apple-darwin.tar.gz`, exe: "avu" };
  if (platform === "win32") return { asset: `avu-${cpu}-pc-windows-msvc.zip`, exe: "avu.exe" };
  throw new Error(`Unsupported platform: ${platform}/${arch}`);
}

function releaseUrl(asset) {
  if (version === "latest") return `${repo}/releases/latest/download/${asset}`;
  return `${repo}/releases/download/v${version}/${asset}`;
}

function download(url, output) {
  return new Promise((resolve, reject) => {
    const request = get(url, response => {
      if ([301, 302, 303, 307, 308].includes(response.statusCode || 0) && response.headers.location) {
        download(response.headers.location, output).then(resolve, reject);
        return;
      }
      if (response.statusCode !== 200) {
        reject(new Error(`Download failed: HTTP ${response.statusCode} ${url}`));
        return;
      }
      const file = createWriteStream(output);
      response.pipe(file);
      file.on("finish", () => file.close(resolve));
      file.on("error", reject);
    });
    request.on("error", reject);
  });
}

function sha256(file) {
  return new Promise((resolve, reject) => {
    const hash = createHash("sha256");
    const stream = createReadStream(file);
    stream.on("data", chunk => hash.update(chunk));
    stream.on("end", () => resolve(hash.digest("hex")));
    stream.on("error", reject);
  });
}

async function verifyArchive(archive, asset, checksumsPath) {
  if (!existsSync(checksumsPath)) return;
  const checksums = readFileSync(checksumsPath, "utf8").split(/\r?\n/);
  const line = checksums.find(entry => {
    const parts = entry.trim().split(/\s+/);
    return parts[1] === asset;
  });
  if (!line) throw new Error(`SHA256SUMS did not contain ${asset}`);
  const expected = line.trim().split(/\s+/)[0].toLowerCase();
  const actual = (await sha256(archive)).toLowerCase();
  if (expected !== actual) {
    throw new Error(`Checksum mismatch for ${asset}: expected ${expected}, got ${actual}`);
  }
}

function run(command, args) {
  const result = spawnSync(command, args, { stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} ${args.join(" ")} failed`);
}

async function downloadBinary() {
  const selected = target();
  const nativeDir = join(__dirname, "native");
  const archive = join(nativeDir, selected.asset);
  const checksums = join(nativeDir, "SHA256SUMS");
  const destination = join(nativeDir, selected.exe);

  mkdirSync(nativeDir, { recursive: true });
  if (existsSync(destination)) rmSync(destination, { force: true });

  console.error(`Downloading Avu ${version} for ${process.platform}/${process.arch}...`);
  await download(releaseUrl(selected.asset), archive);

  try {
    await download(releaseUrl("SHA256SUMS"), checksums);
    await verifyArchive(archive, selected.asset, checksums);
  } catch (err) {
    console.error(`Checksum verification skipped: ${err.message}`);
  }

  if (selected.asset.endsWith(".zip")) {
    run("powershell", ["-NoProfile", "-Command", `Expand-Archive -Force '${archive}' '${nativeDir}'`]);
  } else {
    run("tar", ["-xzf", archive, "-C", nativeDir]);
  }

  if (!existsSync(destination)) throw new Error(`Archive did not contain ${selected.exe}`);
  if (process.platform !== "win32") chmodSync(destination, 0o755);
  rmSync(archive, { force: true });
  if (existsSync(checksums)) rmSync(checksums, { force: true });
}

async function main() {
  const nativeDir = join(__dirname, "native");
  const executable = process.platform === "win32" ? "avu.exe" : "avu";
  const binary = join(nativeDir, executable);

  if (!existsSync(binary)) {
    try {
      await downloadBinary();
    } catch (err) {
      console.error("Failed to download Avu binary:");
      console.error(err.message);
      console.error("Install Hermes or OpenClaw first so their supported Node/npm runtime is available, then retry.");
      console.error("Or build from source: cargo build --release --target ...");
      process.exit(1);
    }
  }

  const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });
  if (result.error) {
    console.error(result.error.message);
    process.exit(1);
  }
  process.exit(result.status ?? 0);
}

main();
