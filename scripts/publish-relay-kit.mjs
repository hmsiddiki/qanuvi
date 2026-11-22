#!/usr/bin/env node
// Relay kit K5: signs a built kit (scripts/build-relay-kit.sh) and puts it on an update channel beside latest.json.
//
//   node scripts/publish-relay-kit.mjs <qanuvi-relay-kit-linux-x86_64.tar.gz>
//
// Signed with the update key (QANUVI_UPDATER_KEY, else %APPDATA%\Qanuvi-publisher\updater.key), so anyone can check
// the download with the public update key. Copied with its .sha256 and .sig to QANUVI_UPDATES_DIR (else the dev
// channel folder). Publishing it is an owner step: ask first.
import { execSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const [kit] = process.argv.slice(2);
if (!kit || !fs.existsSync(kit) || !kit.endsWith(".tar.gz")) {
  console.error("Usage: node scripts/publish-relay-kit.mjs <qanuvi-relay-kit-linux-x86_64.tar.gz>");
  process.exit(1);
}
const root = path.resolve(path.dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Z]:)/i, "$1")), "..");
const keyFile =
  process.env.QANUVI_UPDATER_KEY || path.join(process.env.APPDATA ?? "", "Qanuvi-publisher", "updater.key");
const outDir = process.env.QANUVI_UPDATES_DIR || "E:/Wnmp/nginx-bins/default/www/qanuvi-updates/dev";
if (!fs.existsSync(keyFile)) {
  console.error(`The private update key is missing: ${keyFile}`);
  process.exit(1);
}

const name = path.basename(kit);
const bytes = fs.readFileSync(kit);
const sum = crypto.createHash("sha256").update(bytes).digest("hex");
const sumFile = `${kit}.sha256`;
if (fs.existsSync(sumFile) && !fs.readFileSync(sumFile, "utf8").startsWith(sum)) {
  console.error("The kit does not match its .sha256 (copied badly?).");
  process.exit(1);
}

fs.mkdirSync(outDir, { recursive: true });
const out = path.join(outDir, name);
fs.writeFileSync(out, bytes);
fs.writeFileSync(`${out}.sha256`, `${sum}  ${name}\n`);
execSync(`npx tauri signer sign -f "${keyFile}" -p "${process.env.QANUVI_UPDATER_KEY_PASSWORD ?? ""}" "${out}"`, {
  cwd: root,
  stdio: "ignore",
});
console.log(`Kit ${name} (${(bytes.length / 1048576).toFixed(1)} MB) signed into ${outDir} (+ .sha256, .sig).`);
