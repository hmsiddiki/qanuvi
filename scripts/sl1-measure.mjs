// sl1: time per call, master A called directly (--stdio) against master B -> relay -> A (remote_call), with two
// scratch masters on this machine. Usage: node scripts/sl1-measure.mjs <qanuvi-master> <scratch folder> [relay]
// The scratch folder is deleted at the start and the end; nothing else is touched.
import { spawn, execFileSync } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
const [bin, root, relay = ""] = process.argv.slice(2);
const A = `${root}/a`, B = `${root}/b`, P = `${root}/proj`;
fs.rmSync(root, { recursive: true, force: true });
for (const d of [A, B, P]) fs.mkdirSync(d, { recursive: true });
for (let i = 0; i < 40; i++) fs.writeFileSync(`${P}/f${i}.txt`, `file ${i}\n`);
fs.writeFileSync(`${P}/big.txt`, "x".repeat(200_000));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const now = () => Number(process.hrtime.bigint()) / 1e6;

function bridge(data) {
  const io = spawn(bin, ["--stdio", "--data", data]);
  let buf = "", waiting = new Map(), id = 0;
  io.stdout.on("data", (d) => {
    buf += d; let i;
    while ((i = buf.indexOf("\n")) >= 0) {
      const line = buf.slice(0, i); buf = buf.slice(i + 1);
      try { const m = JSON.parse(line); if (waiting.has(m.id)) { waiting.get(m.id)(m); waiting.delete(m.id); } } catch {}
    }
  });
  const call = (cmd, args = {}) => new Promise((res) => { const n = ++id; waiting.set(n, res); io.stdin.write(JSON.stringify({ id: n, cmd, args }) + "\n"); });
  return { io, call };
}
const stats = (xs) => { const s = [...xs].sort((a, b) => a - b); const q = (p) => s[Math.min(s.length - 1, Math.floor(p * s.length))]; return `median ${q(0.5).toFixed(0)} ms, p90 ${q(0.9).toFixed(0)}, min ${s[0].toFixed(0)}, max ${s[s.length - 1].toFixed(0)}`; };
async function tcpRtt(host, port) {
  const xs = [];
  for (let i = 0; i < 8; i++) {
    const t = now();
    await new Promise((res, rej) => { const s = net.connect(port, host, () => { s.destroy(); res(); }); s.on("error", rej); });
    xs.push(now() - t);
  }
  return stats(xs);
}

const sa = spawn(bin, ["serve", "--data", A], { stdio: "ignore" });
const sb = spawn(bin, ["serve", "--data", B], { stdio: "ignore" });
await sleep(3000);
const a = bridge(A), b = bridge(B);
let serverId = "", pairing = "";
try {
  const pid = (await a.call("projects_add", { path: P })).ok;
  const projectId = pid?.id ?? pid;
  await a.call("relay_configure", { relay, name: "sl1 test A", enabled: true });
  let st;
  for (let i = 0; i < 30; i++) { st = (await a.call("relay_status")).ok; if (st?.connected) break; await sleep(500); }
  console.log("A on relay:", st?.connected, "road:", st?.road);
  const code = (await a.call("relay_pair_start")).ok;
  const c = Array.isArray(code) ? code[0] : code?.code ?? code;
  const args = ["pair", "--code", String(c), "--name", "sl1 test B", "--data", B];
  if (relay) args.push("--relay", relay);
  console.log("B pair:", execFileSync(bin, args, { encoding: "utf8", timeout: 60000 }).trim().split("\n")[0]);
  const list = (await b.call("servers_list")).ok;
  const srv = list.servers.find((s) => s.target === "relay");
  serverId = srv.id; pairing = srv.relay?.pairing ?? "";
  for (let i = 0; i < 30; i++) { const r = await b.call("remote_call", { master: serverId, cmd: "hello", args: {} }); if (r.ok) break; await sleep(500); }

  const host = (st?.relay || relay || "wss://relay.qanuvi.com").replace(/^wss?:\/\//, "").split(/[/:#]/)[0];
  console.log(`TCP connect to ${host}:443 (one network round trip):`, await tcpRtt(host, 443));

  const cases = [
    ["hello", {}],
    ["projects_list", {}],
    ["files_list_dir", { projectId, path: "" }],
    ["files_read", { projectId, path: "f1.txt" }],
    ["files_read", { projectId, path: "big.txt" }],
    ["git_status", { projectId }],
  ];
  const N = 15;
  for (const [cmd, args] of cases) {
    const direct = [], relayed = [];
    for (let i = 0; i < N; i++) {
      let t = now(); const d = await a.call(cmd, args); direct.push(now() - t);
      t = now(); const r = await b.call("remote_call", { master: serverId, cmd, args }); relayed.push(now() - t);
      if (i === 0 && r.error) console.log(`  ${cmd} error over relay: ${r.error}`);
      if (i === 0 && d.error) console.log(`  ${cmd} error direct: ${d.error}`);
      await sleep(30 + Math.random() * 60);
    }
    const label = cmd + (args.path ? ` ${args.path}` : "");
    console.log(`${label.padEnd(22)} direct: ${stats(direct)}\n${"".padEnd(22)} relay:  ${stats(relayed)}`);
  }
  // Ten at once (a switch that sends its calls together).
  for (let round = 0; round < 3; round++) {
    const t = now();
    await Promise.all(Array.from({ length: 10 }, () => b.call("remote_call", { master: serverId, cmd: "projects_list", args: {} })));
    console.log(`10 calls at once over relay: ${(now() - t).toFixed(0)} ms`);
  }
  // Ten one after another (a switch that waits for each).
  const t = now();
  for (let i = 0; i < 10; i++) await b.call("remote_call", { master: serverId, cmd: "projects_list", args: {} });
  console.log(`10 calls one after another over relay: ${(now() - t).toFixed(0)} ms`);
} catch (e) {
  console.log("FAILED:", e?.stack ?? e);
} finally {
  try { if (serverId) await b.call("servers_remove", { id: serverId }); } catch {}
  try { if (pairing) await b.call("remote_unpair", { pairing }); } catch {}
  try { await a.call("relay_configure", { relay, name: "sl1 test A", enabled: false }); } catch {}
  a.io.stdin.end(); b.io.stdin.end(); a.io.kill(); b.io.kill();
  for (const d of [A, B]) { try { execFileSync(bin, ["stop", "--data", d], { encoding: "utf8" }); } catch {} }
  sa.kill(); sb.kill();
  await sleep(1500);
  fs.rmSync(root, { recursive: true, force: true });
  console.log("cleaned up");
}
