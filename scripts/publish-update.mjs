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
// 4. M8a: master builds, signed with the same key, listed beside the installer as master-<os>-<arch>: the Windows one
//    the build above made, and the Linux server build (cargo zigbuild; QANUVI_SKIP_LINUX_MASTER=1 leaves it out). A
//    macOS master build is made on a Mac; until then macOS masters say the version has no build for them yet.
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

// 0. The release rule: Claude Code and its models are current before a release (QANUVI_RELEASE_ANYWAY=1 skips it).
if (!process.env.QANUVI_RELEASE_ANYWAY) {
  const problems = await releaseCheck(root);
  if (problems.length) {
    console.error("Not released: first bring these up to date (or set QANUVI_RELEASE_ANYWAY=1):");
    for (const p of problems) console.error(" - " + p);
    process.exit(2);
  }
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
const platforms = { "windows-x86_64": { signature, url: `${baseUrl}/${name}` } };

// 4. The master builds (M8a): each copied to the channel with its signature and listed for its kind of machine.
const masters = [["master-windows-x86_64", path.join(root, "src-tauri/target/release/qanuvi-master.exe"), ".exe"]];
if (process.env.QANUVI_SKIP_LINUX_MASTER) {
  console.warn("(No Linux master build this time: QANUVI_SKIP_LINUX_MASTER is set.)");
} else {
  // glibc 2.36, so the build runs on older servers too; zig from QANUVI_ZIG_DIR (else where it is installed here).
  const target = "x86_64-unknown-linux-gnu";
  const zig = process.env.QANUVI_ZIG_DIR || "E:/DevTools/zig/zig-windows-x86_64-0.13.0";
  execSync(`cargo zigbuild --release --no-default-features --bin qanuvi-master --target ${target}.2.36 -j 2`, {
    cwd: path.join(root, "src-tauri"),
    stdio: "inherit",
    env: { ...process.env, PATH: `${zig}${path.delimiter}${process.env.PATH}` },
  });
  masters.push(["master-linux-x86_64", path.join(root, `src-tauri/target/${target}/release/qanuvi-master`), ""]);
}
for (const [key, file, ext] of masters) {
  if (!fs.existsSync(file)) {
    console.error(`The ${key} build is missing: ${file}`);
    process.exit(1);
  }
  const out = `qanuvi-master_${version}_${key.replace(/^master-/, "")}${ext}`;
  fs.copyFileSync(file, path.join(outDir, out));
  platforms[key] = { signature: signFile(path.join(outDir, out)), url: `${baseUrl}/${out}` };
  console.log(`Master build ${key}: ${out}`);
}

const latest = {
  version,
  notes: notes || `Qanuvi ${version}`,
  pub_date: new Date().toISOString(),
  platforms,
};
fs.writeFileSync(path.join(outDir, "latest.json"), JSON.stringify(latest, null, 2) + "\n");
console.log(`Published ${name} and latest.json to ${outDir} (${baseUrl}/latest.json)`);

/** Signs a file with the update key (Tauri's signer writes <file>.sig beside it) and answers the signature. */
function signFile(file) {
  execSync(`npx tauri signer sign -f "${keyFile}" -p "${process.env.QANUVI_UPDATER_KEY_PASSWORD ?? ""}" "${file}"`, {
    cwd: root,
    stdio: "ignore",
  });
  return fs.readFileSync(`${file}.sig`, "utf8").trim();
}

/** What is out of date before a release: Claude Code itself, and models it knows that Qanuvi does not offer yet. */
async function releaseCheck(root) {
  const problems = [];
  let installed = "";
  try {
    installed = (execSync("claude --version", { encoding: "utf8" }).match(/\d+\.\d+\.\d+/) ?? [""])[0];
  } catch {
    problems.push("Claude Code is not installed on this computer, so its version and models cannot be checked.");
    return problems;
  }
  try {
    const res = await fetch("https://registry.npmjs.org/@anthropic-ai/claude-code/latest");
    const latest = (await res.json()).version;
    const n = (v) => v.split(".").map(Number);
    const [a, b] = [n(latest), n(installed)];
    if (a[0] > b[0] || (a[0] === b[0] && (a[1] > b[1] || (a[1] === b[1] && a[2] > b[2]))))
      problems.push(`Claude Code ${latest} is out (here ${installed}): run "claude update", then publish again.`);
  } catch {
    console.warn("(The newest Claude Code version could not be looked up; going on.)");
  }
  // The models the installed Claude Code knows, by family, against the ones Qanuvi offers.
  try {
    const exe = execSync(process.platform === "win32" ? "where claude" : "command -v claude", { encoding: "utf8" })
      .split(/\r?\n/)[0]
      .trim();
    const text = fs.readFileSync(fs.realpathSync(exe)).toString("latin1");
    const known = new Set(text.match(/claude-(fable|opus|sonnet|haiku)-\d+(?:-\d+)?(?![\w.-])/g) ?? []);
    const providers = fs.readFileSync(path.join(root, "src/lib/agents/providers.ts"), "utf8");
    const offered = new Set(providers.match(/claude-(fable|opus|sonnet|haiku)-\d+(?:-\d+)?/g) ?? []);
    const ver = (id) => id.split("-").slice(2).map(Number);
    const newer = (x, y) => {
      const [a, b] = [ver(x), ver(y)];
      for (let i = 0; i < Math.max(a.length, b.length); i++)
        if ((a[i] ?? 0) !== (b[i] ?? 0)) return (a[i] ?? 0) > (b[i] ?? 0);
      return false;
    };
    for (const family of ["fable", "opus", "sonnet", "haiku"]) {
      const best = (set) =>
        [...set].filter((m) => m.startsWith(`claude-${family}-`)).reduce((t, m) => (!t || newer(m, t) ? m : t), "");
      const k = best(known);
      const o = best(offered);
      if (k && (!o || newer(k, o)))
        problems.push(
          `Claude Code knows ${k}, but Qanuvi offers ${o || "no " + family + " model"}: add it to src/lib/agents/providers.ts (and the knowledge file).`,
        );
    }
  } catch {
    console.warn("(Claude Code's models could not be read; going on.)");
  }
  return problems;
}
