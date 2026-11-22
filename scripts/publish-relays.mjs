#!/usr/bin/env node
// 24rl F1: signs the free relay list and puts it on an update channel beside latest.json.
//
//   node scripts/publish-relays.mjs <list.json>
//
// The list is {"relays": [{"url": "wss://...", "name": "...", "runBy": "...", "region": "...", "note": "..."}]}.
// Only relays the owner has checked go in (they run the released qanuvi-relay program). The file is signed with the
// update key (QANUVI_UPDATER_KEY, else %APPDATA%\Qanuvi-publisher\updater.key) and copied with its signature to the
// channel folder (QANUVI_UPDATES_DIR, else the dev channel folder). Apps refuse a list that does not match its
// signature and keep their last good copy.
import { execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const [listFile] = process.argv.slice(2);
if (!listFile || !fs.existsSync(listFile)) {
  console.error("Usage: node scripts/publish-relays.mjs <list.json>");
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

// The same checks the app makes, so a list that apps would cut down is caught here.
const list = JSON.parse(fs.readFileSync(listFile, "utf8"));
if (!Array.isArray(list.relays)) {
  console.error('The list needs a "relays" array.');
  process.exit(1);
}
const seen = new Set();
for (const r of list.relays) {
  const url = String(r.url ?? "")
    .trim()
    .replace(/\/+$/, "");
  if (!url.startsWith("wss://") || url.length <= 6) throw new Error(`Not a wss:// relay address: ${r.url}`);
  if (url.toLowerCase() === "wss://relay.qanuvi.com") throw new Error("Qanuvi's own relay is not a free relay.");
  if (seen.has(url.toLowerCase())) throw new Error(`Listed twice: ${url}`);
  if (!String(r.name ?? "").trim()) throw new Error(`No name for ${url}`);
  seen.add(url.toLowerCase());
}

fs.mkdirSync(outDir, { recursive: true });
const out = path.join(outDir, "relays.json");
fs.writeFileSync(out, JSON.stringify(list, null, 2) + "\n");
execSync(`npx tauri signer sign -f "${keyFile}" -p "${process.env.QANUVI_UPDATER_KEY_PASSWORD ?? ""}" "${out}"`, {
  cwd: root,
  stdio: "ignore",
});
console.log(`Signed ${list.relays.length} free relay(s) into ${out} (+ .sig).`);
