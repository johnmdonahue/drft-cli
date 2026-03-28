#!/usr/bin/env node

// Downloads the correct drft binary for the current platform from GitHub Releases.
// Runs as a postinstall script.

const { execSync } = require("child_process");
const fs = require("fs");
const path = require("path");
const https = require("https");

const VERSION = require("./package.json").version;
const REPO = "johnmdonahue/drft-cli";

const PLATFORMS = {
  "darwin-arm64": `drft-cli-aarch64-apple-darwin.tar.xz`,
  "darwin-x64": `drft-cli-x86_64-apple-darwin.tar.xz`,
  "linux-arm64": `drft-cli-aarch64-unknown-linux-gnu.tar.xz`,
  "linux-x64": `drft-cli-x86_64-unknown-linux-gnu.tar.xz`,
  "win32-x64": `drft-cli-x86_64-pc-windows-msvc.zip`,
};

function getPlatformKey() {
  const platform = process.platform;
  const arch = process.arch;
  return `${platform}-${arch}`;
}

function getDownloadUrl(assetName) {
  return `https://github.com/${REPO}/releases/download/v${VERSION}/${assetName}`;
}

function download(url) {
  return new Promise((resolve, reject) => {
    const follow = (url) => {
      https.get(url, { headers: { "User-Agent": "drft-npm" } }, (res) => {
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
          follow(res.headers.location);
          return;
        }
        if (res.statusCode !== 200) {
          reject(new Error(`Download failed: HTTP ${res.statusCode} from ${url}`));
          return;
        }
        const chunks = [];
        res.on("data", (chunk) => chunks.push(chunk));
        res.on("end", () => resolve(Buffer.concat(chunks)));
        res.on("error", reject);
      }).on("error", reject);
    };
    follow(url);
  });
}

async function main() {
  const platformKey = getPlatformKey();
  const assetName = PLATFORMS[platformKey];

  if (!assetName) {
    console.error(
      `drft: unsupported platform ${platformKey}. ` +
      `Supported: ${Object.keys(PLATFORMS).join(", ")}. ` +
      `Install from source: cargo install drft-cli`
    );
    process.exit(0); // Don't fail the install — just won't have the binary
  }

  const binDir = path.join(__dirname, "bin");
  const binPath = path.join(binDir, process.platform === "win32" ? "drft.exe" : "drft");

  // Skip if binary already exists (e.g., reinstall)
  if (fs.existsSync(binPath)) {
    return;
  }

  const url = getDownloadUrl(assetName);
  console.log(`drft: downloading ${platformKey} binary...`);

  try {
    const data = await download(url);

    // Write archive to temp file
    const tmpFile = path.join(binDir, assetName);
    fs.writeFileSync(tmpFile, data);

    // Extract
    if (assetName.endsWith(".tar.xz")) {
      execSync(`tar xf "${tmpFile}" -C "${binDir}" --strip-components=1`, { stdio: "pipe" });
    } else if (assetName.endsWith(".zip")) {
      execSync(`unzip -o "${tmpFile}" -d "${binDir}"`, { stdio: "pipe" });
    }

    // Clean up archive
    fs.unlinkSync(tmpFile);

    // Ensure executable
    if (process.platform !== "win32") {
      fs.chmodSync(binPath, 0o755);
    }

    console.log("drft: installed successfully");
  } catch (err) {
    console.error(
      `drft: failed to download binary (${err.message}). ` +
      `Install from source: cargo install drft-cli`
    );
    process.exit(0); // Don't fail the install
  }
}

main();
