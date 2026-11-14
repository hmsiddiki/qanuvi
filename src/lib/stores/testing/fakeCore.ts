/**
 * A fake Qanuvi core for driving the real plan runner in tests (runner.scenarios.test.ts). Every api.* call ends in
 * one `invoke`; this answers them from memory: project files, Qanuvi's snapshots, agent sessions run by a script,
 * checks and run records. An unknown command fails loudly, so a new dependency of the runner shows up here first.
 */
import type { AgentEvent, Session } from "$lib/api";

export interface AgentCall {
  prompt: string;
  model: string;
  agent: string;
  sessionId: string;
  /** Turns sent to this session so far (1 = the first). */
  turn: number;
  core: FakeCore;
}

/** What the scripted agent did: the session's end status and cost (defaults: idle, $0.01). */
export interface AgentReply {
  status?: "idle" | "failed";
  cost?: number;
}

interface File {
  text: string;
  hash: string;
  at: number;
}

export class FakeCore {
  files = new Map<string, File>();
  sessions = new Map<string, Session>();
  runs = new Map<
    string,
    { id: string; file: string; status: string; data: string; created_at: number; updated_at: number }
  >();
  snapshots = new Map<string, Map<string, string>>();
  /** Every command the runner sent, in order. */
  calls: string[] = [];
  /** Prompts sent to agents, in order. */
  prompts: { model: string; prompt: string }[] = [];
  /** The scripted agent: edits files through `core`, returns how the turn ended. */
  agent: (c: AgentCall) => AgentReply | void = () => {};
  /** The test command's answer. */
  checks: (command: string) => { ok: boolean; output: string } = () => ({ ok: true, output: "" });
  /** Simulates a core whose diff sees nothing (what git did in a folder it ignores, PLAN-F 2026-11-14). */
  blindDiff = false;
  /** Plan folders no git repository sees (reported with each snapshot). */
  gitBlind: string[] = [];
  #n = 0;
  #turns = new Map<string, number>();

  write(path: string, text: string): void {
    this.files.set(path, { text, hash: `h${++this.#n}`, at: Date.now() });
  }

  read(path: string): string {
    return this.files.get(path)?.text ?? "";
  }

  /** Ticks (or marks) a step by its number in a plan file, as an agent does. */
  mark(file: string, number: string, status: "x" | "!" | " " = "x"): void {
    const lines = this.read(file).split("\n");
    const i = lines.findIndex((l) =>
      new RegExp(`^ ?- \\[[ xX!-]\\] \\*\\*${number.replace(/[-]/g, "\\-")}\\.\\s`).test(l),
    );
    if (i < 0) throw new Error(`fake core: step ${number} is not in ${file}`);
    lines[i] = lines[i].replace(/^( ?- \[)[ xX!-](\])/, `$1${status}$2`);
    this.write(file, lines.join("\n"));
  }

  #own(path: string): boolean {
    return !path.startsWith(".qanuvi/");
  }

  #session(id: string, patch: Partial<Session>): Session {
    const now = Date.now();
    const s: Session = {
      id,
      project_id: "p1",
      title: "",
      agent: "claude-code",
      model: "",
      mode: "auto",
      status: "running",
      agent_session_id: null,
      input_tokens: 0,
      output_tokens: 0,
      cache_read_tokens: 0,
      cost_usd: 0,
      duration_ms: 0,
      created_at: now,
      updated_at: now,
      ...this.sessions.get(id),
      ...patch,
    };
    this.sessions.set(id, s);
    return s;
  }

  async invoke(cmd: string, a: Record<string, unknown> = {}): Promise<unknown> {
    this.calls.push(cmd);
    const path = String(a.path ?? "");
    switch (cmd) {
      case "files_read": {
        const f = this.files.get(path);
        if (!f) throw `${path} cannot be found.`;
        return { path, text: f.text, hash: f.hash, size: f.text.length, binary: false, too_large: false };
      }
      case "files_write": {
        const f = this.files.get(path);
        if (a.expectedHash && f && f.hash !== a.expectedHash) throw `${path} changed on disk since it was read.`;
        this.write(path, String(a.text));
        return this.files.get(path)!.hash;
      }
      case "files_log_append":
        this.write(path, this.read(path) + String(a.text));
        return null;
      case "files_changed_since": {
        const since = Number(a.sinceMs);
        return [...this.files]
          .filter(([p, f]) => this.#own(p) && f.at >= since)
          .map(([p]) => p)
          .sort();
      }
      case "snapshots_take": {
        const id = `snap${++this.#n}`;
        this.snapshots.set(id, new Map([...this.files].filter(([p]) => this.#own(p)).map(([p, f]) => [p, f.text])));
        return {
          id,
          files: this.snapshots.get(id)!.size,
          stored: 0,
          truncated: false,
          ms: 0,
          git_blind: this.gitBlind,
        };
      }
      case "snapshots_changes": {
        const before = this.snapshots.get(String(a.before));
        if (!before) throw "The snapshot is gone (older than two weeks, or the data folder moved).";
        const after = (await this.invoke("snapshots_take", {})) as { id: string };
        if (this.blindDiff) return { changes: [], unseen: [], after: after.id };
        const now = this.snapshots.get(after.id)!;
        const changes = [];
        for (const p of [...new Set([...before.keys(), ...now.keys()])].sort()) {
          const [b, n] = [before.get(p), now.get(p)];
          if (b === n) continue;
          const kind = b === undefined ? "added" : n === undefined ? "deleted" : "changed";
          const diff = [
            `--- ${p} (before this step)`,
            `+++ ${p} (after)`,
            ...(b ? b.split("\n").map((l) => `-${l}`) : []),
            ...(n ? n.split("\n").map((l) => `+${l}`) : []),
          ].join("\n");
          changes.push({ path: p, kind, diff });
        }
        const unseen = a.git
          ? changes.map((c) => c.path).filter((p) => this.gitBlind.some((d) => p.startsWith(`${d}/`)))
          : [];
        return { changes, unseen, after: after.id };
      }
      case "agent_start": {
        const id = (a.sessionId as string | null) ?? `s${++this.#n}`;
        const turn = (this.#turns.get(id) ?? 0) + 1;
        this.#turns.set(id, turn);
        const model = String(a.model);
        const prompt = String(a.prompt);
        this.prompts.push({ model, prompt });
        const s = this.#session(id, { status: "running", model, agent: String(a.agent) });
        const channel = a.onEvent as { onmessage?: (e: AgentEvent) => void } | undefined;
        setTimeout(() => {
          const reply = this.agent({ prompt, model, agent: String(a.agent), sessionId: id, turn, core: this }) ?? {};
          const done = this.#session(id, {
            status: reply.status ?? "idle",
            cost_usd: (this.sessions.get(id)?.cost_usd ?? 0) + (reply.cost ?? 0.01),
          });
          channel?.onmessage?.({ kind: "session", session: done });
        }, 0);
        return s;
      }
      case "agent_stop": {
        const id = String(a.sessionId);
        if (this.sessions.has(id)) this.#session(id, { status: "stopped" });
        return null;
      }
      case "agent_session_events":
      case "plan_private":
      case "index_find":
      case "index_outline":
        return [];
      case "plan_private_add":
      case "plan_private_clear":
        return null;
      case "git_diff":
        return "";
      case "checks_run": {
        const r = this.checks(String(a.command));
        return { ok: r.ok, exit_code: r.ok ? 0 : 1, timed_out: false, duration_ms: 1, output: r.output };
      }
      case "agent_usage_since":
        return { cost_usd: 0, sessions: 0, input_tokens: 0, output_tokens: 0 };
      case "runs_save": {
        const id = String(a.id);
        const now = Date.now();
        const was = this.runs.get(id);
        this.runs.set(id, {
          id,
          file: String(a.file),
          status: String(a.status),
          data: String(a.data),
          created_at: was?.created_at ?? now,
          updated_at: now,
        });
        return null;
      }
      case "runs_list":
      case "runs_list_all":
        return [...this.runs.values()];
      default:
        throw new Error(`fake core: unknown command ${cmd}`);
    }
  }
}
