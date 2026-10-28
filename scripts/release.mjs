#!/usr/bin/env node
// Bump the app version everywhere, commit, and tag a release.
//
//   npm run release -- 2.1
//
// Versions are major.minor (the app shows "v2.1"); the patch part stays 0
// because the updater compares full semver. Pushing the tag starts the
// Release workflow, which publishes the installer and the updater manifest.

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

const arg = (process.argv[2] ?? "").replace(/^v/, "");
if (!/^\d+\.\d+$/.test(arg)) {
  console.error("Usage: npm run release -- <major.minor>   (e.g. 2.1)");
  process.exit(1);
}
const version = `${arg}.0`;
const tag = `v${arg}`;

const run = (cmd, args) => execFileSync(cmd, args, { stdio: "inherit" });
const git = (...args) => execFileSync("git", args, { encoding: "utf8" }).trim();

if (git("status", "--porcelain")) {
  console.error("Commit or stash your changes first.");
  process.exit(1);
}
if (git("tag", "--list", tag)) {
  console.error(`Tag ${tag} already exists.`);
  process.exit(1);
}

function rewrite(path, from, to) {
  const before = readFileSync(path, "utf8");
  const after = before.replace(from, to);
  if (after === before) throw new Error(`version not found in ${path}`);
  writeFileSync(path, after);
}

rewrite("package.json", /("version":\s*")[^"]+(")/, `$1${version}$2`);
rewrite("src-tauri/tauri.conf.json", /("version":\s*")[^"]+(")/, `$1${version}$2`);
rewrite("src-tauri/Cargo.toml", /^(version\s*=\s*")[^"]+(")/m, `$1${version}$2`);
run("npm", ["install", "--package-lock-only", "--ignore-scripts", "--silent"]);
run("cargo", ["update", "--workspace", "--offline"]);

run("git", ["add", "package.json", "package-lock.json", "src-tauri/tauri.conf.json", "src-tauri/Cargo.toml", "Cargo.lock"]);
run("git", ["commit", "-m", `release: ${tag}`]);
run("git", ["tag", "-a", tag, "-m", `Streamy Desktop ${tag}`]);

console.log(`\nTagged ${tag}. Publish it with:\n  git push origin HEAD ${tag}`);
