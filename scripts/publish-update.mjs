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
// 5. L2: the Linux app (linux-x86_64): the app and its master, built on a Linux machine over ssh from this very tree
//    (QANUVI_LINUX_APP_HOST, user@host; QANUVI_LINUX_APP_KEY, its ssh key; QANUVI_LINUX_APP_DIR, the build folder there,
//    else ~/qanuvi-release), packed as a tarball and signed here (the key never leaves this computer). The installed
//    Linux app puts it into the user's home folder. Without QANUVI_LINUX_APP_HOST it is left out, with a warning.
import { execSync, spawnSync } from "node:child_process";
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

// 5. The Linux app (L2), built on a Linux machine from this tree.
const linuxHost = process.env.QANUVI_LINUX_APP_HOST;
if (!linuxHost) {
  console.warn("(No Linux app build this time: QANUVI_LINUX_APP_HOST is not set.)");
} else {
  const out = `Qanuvi_${version}_linux.tar.gz`;
  fs.copyFileSync(linuxApp(linuxHost, version, out), path.join(outDir, out));
  platforms["linux-x86_64"] = { signature: signFile(path.join(outDir, out)), url: `${baseUrl}/${out}` };
  console.log(`Linux app build: ${out}`);
}

const latest = {
  version,
  notes: notes || `Qanuvi ${version}`,
  pub_date: new Date().toISOString(),
  platforms,
};
fs.writeFileSync(path.join(outDir, "latest.json"), JSON.stringify(latest, null, 2) + "\n");
console.log(`Published ${name} and latest.json to ${outDir} (${baseUrl}/latest.json)`);

/**
 * Builds the Linux app on `host` from this tree (its files as they are now, version included) and answers the
 * tarball brought back here (qanuvi, qanuvi-master and the icon), not yet signed.
 */
function linuxApp(host, version, name) {
  const key = process.env.QANUVI_LINUX_APP_KEY;
  const ssh = ["-o", "BatchMode=yes", "-o", "ConnectTimeout=15", ...(key ? ["-i", key] : [])];
  const dir = process.env.QANUVI_LINUX_APP_DIR || "qanuvi-release";
  const work = path.join(root, "src-tauri/target/linux-app");
  fs.mkdirSync(work, { recursive: true });
  // Paths relative to the folder they are in: tar and scp read "E:\..." as a host called E.
  const files = execSync("git ls-files -co --exclude-standard", { cwd: root, encoding: "utf8" })
    .split(/\r?\n/)
    .filter((p) => p && fs.existsSync(path.join(root, p)));
  fs.writeFileSync(path.join(work, "files.txt"), files.join("\n") + "\n");
  execSync("tar -czf src-tauri/target/linux-app/source.tar.gz -T src-tauri/target/linux-app/files.txt", {
    cwd: root,
    stdio: "inherit",
  });
  const run = (cmd, args, input) => {
    const r = spawnSync(cmd, args, {
      cwd: work,
      stdio: [input === undefined ? "inherit" : "pipe", "inherit", "inherit"],
      input,
    });
    if (r.status !== 0) {
      console.error(`The Linux app build failed (${cmd} ${args.at(-1)}).`);
      process.exit(1);
    }
  };
  run("scp", [...ssh, "source.tar.gz", `${host}:qanuvi-release-source.tar.gz`]);
  // On the build machine: the tree unpacked over the last one (its node_modules and target kept, so it builds only
  // what changed), packages installed again only when package-lock.json changed, then the app and its master.
  const script = [
    "set -e",
    '[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"',
    'export PATH="$HOME/.local/node/bin:$HOME/.cargo/bin:$PATH"',
    `d="$HOME/${dir}"`,
    'mkdir -p "$d" && tar -xzf "$HOME/qanuvi-release-source.tar.gz" -C "$d" && cd "$d"',
    'lock="$(sha256sum package-lock.json)"',
    'if [ ! -d node_modules ] || [ "$(cat .lock-hash 2>/dev/null)" != "$lock" ]; then npm ci; echo "$lock" > .lock-hash; fi',
    "CARGO_BUILD_JOBS=4 npx tauri build --no-bundle",
    "mkdir -p out && cp src-tauri/target/release/qanuvi src-tauri/target/release/qanuvi-master out/",
    "cp src-tauri/icons/128x128.png out/qanuvi.png",
    `tar -czf "$HOME/${name}" -C out qanuvi qanuvi-master qanuvi.png`,
    `out/qanuvi-master version | grep -q "${version}"`,
  ].join("\n");
  run("ssh", [...ssh, host, "bash -s"], script + "\n");
  run("scp", [...ssh, `${host}:${name}`, name]);
  return path.join(work, name);
}

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
