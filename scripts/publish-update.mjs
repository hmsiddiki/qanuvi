#!/usr/bin/env node
// Publishes a new version of Qanuvi to an update channel (1b).
//
//   node scripts/publish-update.mjs <version> "<what is new>"
//
// 1. Sets the version in tauri.conf.json, package.json and Cargo.toml.
// 2. Builds the installer with update files, signed with the private update key (kept outside the repository:
//    QANUVI_UPDATER_KEY, else %APPDATA%\Qanuvi-publisher\updater.key).
// 3. Copies the installer and its signature to the channel's folder on the web server (QANUVI_UPDATES_DIR, else the
//    dev channel folder) and writes latest.json there, which installed copies of Qanuvi read.
import { execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const [version, notes = ""] = process.argv.slice(2);
if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) {
  console.error('Usage: node scripts/publish-update.mjs <version, for example 0.1.2> "<what is new>"');
  process.exit(1);
}
const root = path.resolve(path.dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Z]:)/i, "$1")), "..");
const keyFile =
  process.env.QANUVI_UPDATER_KEY || path.join(process.env.APPDATA ?? "", "Qanuvi-publisher", "updater.key");
const outDir = process.env.QANUVI_UPDATES_DIR || "E:/Wnmp/nginx-bins/default/www/qanuvi-updates/dev";
const baseUrl = process.env.QANUVI_UPDATES_URL || "https://qlopy.top/qanuvi-updates/dev";
if (!fs.existsSync(keyFile)) {
  console.error(`The private update key is missing: ${keyFile}`);
  process.exit(1);
}

// 1. The version, everywhere it is written.
const conf = path.join(root, "src-tauri/tauri.conf.json");
const c = JSON.parse(fs.readFileSync(conf, "utf8"));
const previous = c.version;
c.version = version;
fs.writeFileSync(conf, JSON.stringify(c, null, 2) + "\n");
const pkg = path.join(root, "package.json");
fs.writeFileSync(pkg, fs.readFileSync(pkg, "utf8").replace(/("version":\s*")[^"]+(")/, `$1${version}$2`));
const cargo = path.join(root, "src-tauri/Cargo.toml");
fs.writeFileSync(cargo, fs.readFileSync(cargo, "utf8").replace(/^version = "[^"]+"/m, `version = "${version}"`));
console.log(`Version ${previous} -> ${version}`);

// 2. The signed build.
execSync(`npx prettier --write "${conf}"`, { cwd: root, stdio: "ignore" });
execSync(`npx tauri build --config "{\\"bundle\\":{\\"createUpdaterArtifacts\\":true}}"`, {
  cwd: root,
  stdio: "inherit",
  env: {
    ...process.env,
    TAURI_SIGNING_PRIVATE_KEY: fs.readFileSync(keyFile, "utf8").trim(),
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD: process.env.QANUVI_UPDATER_KEY_PASSWORD ?? "",
  },
});

// 3. The files and latest.json on the channel.
const name = `Qanuvi_${version}_x64-setup.exe`;
const built = path.join(root, "src-tauri/target/release/bundle/nsis", name);
const signature = fs.readFileSync(`${built}.sig`, "utf8").trim();
fs.mkdirSync(outDir, { recursive: true });
fs.copyFileSync(built, path.join(outDir, name));
fs.copyFileSync(`${built}.sig`, path.join(outDir, `${name}.sig`));
const latest = {
  version,
  notes: notes || `Qanuvi ${version}`,
  pub_date: new Date().toISOString(),
  platforms: { "windows-x86_64": { signature, url: `${baseUrl}/${name}` } },
};
fs.writeFileSync(path.join(outDir, "latest.json"), JSON.stringify(latest, null, 2) + "\n");
console.log(`Published ${name} and latest.json to ${outDir} (${baseUrl}/latest.json)`);
