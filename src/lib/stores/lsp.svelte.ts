/**
 * Language servers in the window (15i): one client per project and language, started on the first open file of that
 * language and stopped when unused for a while or when the project closes. Keeps the problems every server reports
 * (the Problems list and the underlines) and the last "find references" result.
 */
import { api, errorText, type LspStatus } from "$lib/api";
import { LspClient } from "$lib/lsp/client";
import {
  fileUri,
  relativePath,
  serverFor,
  severity,
  type Range,
  type ServerId,
  type Severity,
} from "$lib/lsp/protocol";
import { projects } from "./projects.svelte";
import { settings } from "./settings.svelte";
import { switcher } from "./switcher.svelte";

export interface Problem {
  projectId: string;
  path: string;
  range: Range;
  severity: Severity;
  message: string;
  source: string;
  /** The diagnostic as the server sent it: handed back when asking for its quick fixes. */
  raw?: unknown;
}

export interface Place {
  projectId: string;
  path: string;
  line: number;
  column: number;
  /** The line's text, when the file is at hand. */
  text?: string;
}

/** A file being edited, as the store needs it. */
export interface Doc {
  key: string;
  projectId: string;
  path: string;
}

export type BottomView = "terminal" | "problems" | "references" | "debug";

const clientKey = (projectId: string, server: ServerId) => `${projectId}::${server}`;
/** Statuses are kept per machine (This PC is ""). */
const machineKey = (machine: string | null) => machine ?? "";
/** How many of an install's newest output lines are shown while it runs. */
const INSTALL_LINES = 4;
/** A server that ended by itself starts again after this wait, at most RESTARTS times in RESTART_SPAN_MS (M5c). */
export const RESTART_WAIT_MS = 2000;
export const RESTARTS = 3;
export const RESTART_SPAN_MS = 5 * 60_000;

class LspStore {
  /** What each machine has installed (migration M1: a shown master's servers run there). */
  #statuses = $state<Record<string, LspStatus[]>>({});
  /** Problems by file (project id and path). */
  problems = $state<Record<string, Problem[]>>({});
  /** Bumps when any file's problems change (the editor redraws its underlines). */
  problemsChanged = $state(0);
  /** A server to offer for the shown file: install it, not now, never. */
  offer = $state<{ server: ServerId; status: LspStatus; machine: string | null } | null>(null);
  installing = $state<string | null>(null);
  installError = $state("");
  /** The newest output lines of the install under way (shown under its button while the user waits). */
  installLog = $state<string[]>([]);
  /** Server state per project and language, for the status bar. */
  state = $state<Record<string, "starting" | "restarting" | "ready" | "stopped" | "failed">>({});
  /** Why a server last stopped or failed, per machine (finding w3: one machine's line showed on another's screen). */
  #lastErrors = $state<Record<string, string>>({});
  /** The shown machine's line. */
  get lastError(): string {
    return this.#lastErrors[machineKey(switcher.shown)] ?? "";
  }
  #noteError(machine: string | null, text: string): void {
    this.#lastErrors = { ...this.#lastErrors, [machineKey(machine)]: text };
  }
  /** Bumps when coding tools or one server are switched on again: open editors attach their file once more (m2). */
  switchedOn = $state(0);
  references = $state<{ title: string; places: Place[] } | null>(null);
  bottom = $state<BottomView>("terminal");
  /** Applies a WorkspaceEdit in a project (set by lsp/actions.ts, which knows the editor). */
  editApplier: (projectId: string, edit: unknown) => Promise<boolean> = async () => false;

  #clients = new Map<string, LspClient>();
  #lastUsed = new Map<string, number>();
  /** Which tab key has which document open, to close the ones whose tabs closed. */
  #docs = new Map<string, { client: string; uri: string; doc: Doc; text: () => string }>();
  /** The tab keys open at the last sync (a restarted server opens again only the files still open). */
  #open = new Set<string>();
  /** When each client's server ended by itself lately (M5c: at most RESTARTS in RESTART_SPAN, then it stays stopped). */
  #ends = new Map<string, number[]>();
  #timers = new Map<string, ReturnType<typeof setTimeout>>();
  #notNow = new Set<string>();
  #statusLoad = new Map<string, Promise<void>>();
  #statusAt = new Map<string, number>();
  /** The machine of each running client. */
  #machineOf = new Map<string, string | null>();
  /** Clients being started, so a second call for the same one waits (finding w4). */
  #opening = new Map<string, Promise<unknown>>();
  #idle: ReturnType<typeof setInterval> | undefined;

  constructor() {
    // Servers a previous load of this window started (a reload) are not reachable any more.
    if (typeof window !== "undefined") api.lsp.stopWindow().catch(() => {});
  }

  /** The shown machine's servers (the Languages settings). */
  get statuses(): LspStatus[] {
    return this.#statuses[machineKey(switcher.shown)] ?? [];
  }

  loadStatuses(force = false, machine: string | null = switcher.shown): Promise<void> {
    const k = machineKey(machine);
    let load = this.#statusLoad.get(k);
    if (!load || force) {
      this.#statusAt.set(k, Date.now());
      load = api.lsp
        .statuses(machine)
        .then((s) => void (this.#statuses = { ...this.#statuses, [k]: s }))
        .catch(() => {
          this.#statuses = { ...this.#statuses, [k]: [] };
          // Asked again next time (the machine may answer later, or be updated).
          this.#statusLoad.delete(k);
        });
      this.#statusLoad.set(k, load);
    }
    return load;
  }

  status(server: ServerId, machine: string | null = switcher.shown): LspStatus | undefined {
    return (this.#statuses[machineKey(machine)] ?? []).find((s) => s.id === server);
  }

  enabled(server: ServerId): boolean {
    const l = settings.value.languages;
    return !l.toolsOff && !l.off.includes(server);
  }

  /** Why the shown machine runs no server for a file although it has one: its tools, or that server, are off (m1). */
  offFor(path: string): "tools" | "server" | null {
    const server = serverFor(path);
    if (!server) return null;
    const l = settings.value.languages;
    if (l.toolsOff) return "tools";
    return l.off.includes(server) ? "server" : null;
  }

  /** The shown machine's switch for all its coding tools (a small machine keeps them off). */
  setToolsOff(off: boolean): void {
    settings.set("languages", { ...settings.value.languages, toolsOff: off });
    if (off) this.stopMachine(switcher.shown);
    else this.switchedOn++;
  }

  setEnabled(server: ServerId, on: boolean): void {
    const l = settings.value.languages;
    const off = on ? l.off.filter((x) => x !== server) : [...new Set([...l.off, server])];
    settings.set("languages", { ...l, off });
    if (!on) this.stopServer(server);
    else this.switchedOn++;
  }

  /** The project's folder and the machine it is on (its servers run there, migration M1). */
  #place(projectId: string): { root: string; machine: string | null } | null {
    const p = projects.list.find((x) => x.id === projectId);
    return p ? { root: p.path, machine: p.master ?? null } : null;
  }

  /** The client for a file, started if needed; null when there is none (no server, off, not installed). */
  async clientFor(doc: Doc): Promise<{ client: LspClient; uri: string } | null> {
    const server = serverFor(doc.path);
    if (!server) return null;
    // Finding w4: two files of one project opened at once (a window opening on a machine) each started the server
    // while the other was still asking which servers the machine has. The second waits for the first.
    const key = clientKey(doc.projectId, server);
    const going = this.#opening.get(key);
    if (going) await going.catch(() => null);
    const run = this.#clientFor(doc);
    if (!going && !this.#clients.has(key)) {
      this.#opening.set(key, run);
      void run.catch(() => null).finally(() => this.#opening.get(key) === run && this.#opening.delete(key));
    }
    return run;
  }

  async #clientFor(doc: Doc): Promise<{ client: LspClient; uri: string } | null> {
    const server = serverFor(doc.path);
    const place = this.#place(doc.projectId);
    if (!server || !place || !this.enabled(server)) return null;
    const { root, machine } = place;
    const key = clientKey(doc.projectId, server);
    let client = this.#clients.get(key);
    if (!client) {
      await this.loadStatuses(false, machine);
      let st = this.status(server, machine);
      // Installed meanwhile from outside this window (another window, npm or rustup in a terminal): look again,
      // at most every 30 s, since the check starts programs (rust-analyzer --version).
      if (!st?.found && Date.now() - (this.#statusAt.get(machineKey(machine)) ?? 0) > 30_000) {
        await this.loadStatuses(true, machine);
        st = this.status(server, machine);
      }
      if (!st?.found) {
        const l = settings.value.languages;
        if (st && !l.never.includes(server) && !this.#notNow.has(server)) this.offer = { server, status: st, machine };
        return null;
      }
      client = new LspClient(
        doc.projectId,
        server,
        root,
        (why) => {
          if (this.#clients.get(key) !== client) return;
          this.#clients.delete(key);
          this.#machineOf.delete(key);
          const reopen = [...this.#docs.values()].filter((d) => d.client === key);
          this.#forget(key);
          this.#clearProblems(doc.projectId, server);
          // M5c: it ended by itself (it crashed, or the master's link ended): started again for its open files, so
          // the list says "Checking…" meanwhile instead of "No problems found".
          if (reopen.length && this.#mayRestart(key)) {
            this.state = { ...this.state, [key]: "restarting" };
            setTimeout(() => void this.#restart(key, reopen), RESTART_WAIT_MS);
            return;
          }
          this.state = { ...this.state, [key]: "stopped" };
          if (why) this.#noteError(machine, `${st.label} stopped: ${why}`);
        },
        machine,
      );
      this.#clients.set(key, client);
      this.#machineOf.set(key, machine);
      this.state = { ...this.state, [key]: "starting" };
      const c = client;
      c.applyEdit = (edit) => this.editApplier(doc.projectId, edit);
      c.on("textDocument/publishDiagnostics", (p) => this.#diagnostics(doc.projectId, root, p));
      c.ready.then(
        () => {
          if (this.#clients.get(key) !== c) return;
          this.state = { ...this.state, [key]: "ready" };
          // Working again (it came back after its machine's link was made again): an old failure line goes.
          if (this.#lastErrors[machineKey(machine)]?.startsWith(st.label)) this.#noteError(machine, "");
        },
        (e) => {
          this.#clients.delete(key);
          this.state = { ...this.state, [key]: "failed" };
          this.#noteError(machine, `${st.label}: ${errorText(e)}`);
          c.stop();
        },
      );
      this.#watchIdle();
    }
    this.#lastUsed.set(key, Date.now());
    try {
      await client.ready;
    } catch {
      return null;
    }
    return { client, uri: fileUri(root, doc.path) };
  }

  /** The shown file: opened in its server (with its current text). */
  async attach(doc: Doc, text: () => string): Promise<void> {
    const c = await this.clientFor(doc);
    if (!c) return;
    c.client.open(c.uri, doc.path, text());
    this.#docs.set(doc.key, { client: clientKey(doc.projectId, c.client.server), uri: c.uri, doc, text });
  }

  /** Whether a server that ended by itself is started again (fewer than RESTARTS ends in RESTART_SPAN). */
  #mayRestart(key: string): boolean {
    const now = Date.now();
    const recent = (this.#ends.get(key) ?? []).filter((t) => now - t < RESTART_SPAN_MS);
    recent.push(now);
    this.#ends.set(key, recent);
    return recent.length <= RESTARTS;
  }

  async #restart(key: string, docs: { doc: Doc; text: () => string }[]): Promise<void> {
    // Stopped on purpose meanwhile (its project closed, its machine hidden): it stays so.
    if (this.state[key] !== "restarting") return;
    const still = docs.filter((d) => this.#open.has(d.doc.key));
    if (!still.length) {
      const s = { ...this.state };
      delete s[key];
      this.state = s;
      return;
    }
    for (const d of still) await this.attach(d.doc, d.text);
    // Nowhere to start it (its machine is not shown any more, it is not installed there now): it is not waited for.
    if (this.state[key] === "restarting" && !this.#clients.has(key)) {
      const s = { ...this.state };
      delete s[key];
      this.state = s;
    }
  }

  /** The text changed: the server hears of it after a short pause in typing. */
  changed(doc: Doc, text: () => string): void {
    const d = this.#docs.get(doc.key);
    if (!d) return;
    clearTimeout(this.#timers.get(doc.key));
    this.#timers.set(
      doc.key,
      setTimeout(() => {
        this.#timers.delete(doc.key);
        const client = this.#clients.get(d.client);
        if (client?.isOpen(d.uri)) client.change(d.uri, text());
        this.#lastUsed.set(d.client, Date.now());
      }, 250),
    );
  }

  /** Sends a waiting change at once (before asking the server something about the text). */
  flush(doc: Doc, text: string): void {
    const t = this.#timers.get(doc.key);
    if (!t) return;
    clearTimeout(t);
    this.#timers.delete(doc.key);
    const d = this.#docs.get(doc.key);
    const client = d && this.#clients.get(d.client);
    if (d && client?.isOpen(d.uri)) client.change(d.uri, text);
  }

  saved(doc: Doc, text: string): void {
    const d = this.#docs.get(doc.key);
    const client = d && this.#clients.get(d.client);
    if (d && client) client.saved(d.uri, text);
  }

  /** Closes the documents of tabs that are gone. */
  sync(openKeys: string[]): void {
    const open = new Set(openKeys);
    this.#open = open;
    for (const [key, d] of this.#docs) {
      if (open.has(key)) continue;
      this.#docs.delete(key);
      this.#clients.get(d.client)?.close(d.uri);
    }
  }

  #forget(client: string) {
    for (const [key, d] of this.#docs) if (d.client === client) this.#docs.delete(key);
  }

  /** Drops what a server reported (it stopped; its files are checked again when it starts). */
  #clearProblems(projectId: string, server: ServerId) {
    const next = { ...this.problems };
    for (const [k, list] of Object.entries(next))
      if (list[0]?.projectId === projectId && serverFor(list[0].path) === server) delete next[k];
    this.problems = next;
    this.problemsChanged++;
  }

  #diagnostics(projectId: string, root: string, params: unknown) {
    const p = params as {
      uri: string;
      diagnostics: { range: Range; severity?: number; message: string; source?: string; code?: unknown }[];
    };
    const path = relativePath(root, p.uri);
    if (path === null) return;
    const key = `${projectId}::${path}`;
    const list: Problem[] = (p.diagnostics ?? []).map((d) => ({
      raw: d,
      projectId,
      path,
      range: d.range,
      severity: severity(d.severity),
      message: d.message,
      source: [d.source, d.code !== undefined && d.code !== null ? String(d.code) : ""].filter(Boolean).join(" "),
    }));
    const next = { ...this.problems };
    if (list.length) next[key] = list;
    else delete next[key];
    this.problems = next;
    this.problemsChanged++;
  }

  problemsFor(projectId: string, path: string): Problem[] {
    return this.problems[`${projectId}::${path}`] ?? [];
  }

  /** All problems of a project: errors first, then by file and line. */
  projectProblems(projectId: string | null): Problem[] {
    if (!projectId) return [];
    const rank = { error: 0, warning: 1, info: 2, hint: 3 };
    return Object.values(this.problems)
      .flat()
      .filter((p) => p.projectId === projectId && p.severity !== "hint")
      .sort(
        (a, b) =>
          rank[a.severity] - rank[b.severity] ||
          a.path.localeCompare(b.path) ||
          a.range.start.line - b.range.start.line,
      );
  }

  counts(projectId: string | null): { errors: number; warnings: number } {
    const all = this.projectProblems(projectId);
    return {
      errors: all.filter((p) => p.severity === "error").length,
      warnings: all.filter((p) => p.severity === "warning").length,
    };
  }

  // Installing ----------------------------------------------------------------------------------------------------

  notNow(server: ServerId): void {
    this.#notNow.add(server);
    if (this.offer?.server === server) this.offer = null;
  }

  never(server: ServerId): void {
    const l = settings.value.languages;
    settings.set("languages", { ...l, never: [...new Set([...l.never, server])] });
    if (this.offer?.server === server) this.offer = null;
  }

  /** Installs a server (the user clicked Install, which is their yes). */
  async install(
    server: ServerId,
    machine: string | null = this.offer?.server === server ? this.offer.machine : switcher.shown,
  ): Promise<boolean> {
    this.installing = server;
    this.installError = "";
    this.installLog = [];
    try {
      const st = await api.lsp.install(server, machine, (line) => {
        if (this.installing === server) this.installLog = [...this.installLog, line].slice(-INSTALL_LINES);
      });
      const k = machineKey(machine);
      this.#statuses = { ...this.#statuses, [k]: (this.#statuses[k] ?? []).map((s) => (s.id === st.id ? st : s)) };
      if (this.offer?.server === server) this.offer = null;
      return true;
    } catch (e) {
      this.installError = errorText(e);
      return false;
    } finally {
      this.installing = null;
      this.installLog = [];
    }
  }

  // Stopping ------------------------------------------------------------------------------------------------------

  #stop(key: string) {
    const c = this.#clients.get(key);
    if (!c) {
      // One waiting to start again stays stopped.
      if (this.state[key] === "restarting") {
        const s = { ...this.state };
        delete s[key];
        this.state = s;
      }
      return;
    }
    this.#clients.delete(key);
    this.#machineOf.delete(key);
    this.#forget(key);
    this.#clearProblems(c.projectId, c.server);
    const s = { ...this.state };
    delete s[key];
    this.state = s;
    c.stop();
  }

  stopServer(server: ServerId): void {
    for (const key of this.#keys()) if (key.split("::")[1] === server) this.#stop(key);
  }

  stopProject(projectId: string): void {
    for (const key of this.#keys()) if (key.startsWith(`${projectId}::`)) this.#stop(key);
  }

  /** Running clients and the ones waiting to start again. */
  #keys(): string[] {
    const waiting = Object.keys(this.state).filter((k) => this.state[k] === "restarting");
    return [...new Set([...this.#clients.keys(), ...waiting])];
  }

  /** A machine stopped being shown, or its tools were switched off: its servers end (they start again on its next file). */
  stopMachine(machine: string | null): void {
    for (const key of [...this.#clients.keys()]) if ((this.#machineOf.get(key) ?? null) === machine) this.#stop(key);
    if (this.offer?.machine === machine) this.offer = null;
  }

  /** Servers with no open file for a while stop (they start again on the next file). */
  #watchIdle() {
    if (this.#idle) return;
    this.#idle = setInterval(() => {
      const limit = settings.value.languages.idleMinutes * 60_000;
      for (const [key, c] of this.#clients)
        if (c.openCount === 0 && Date.now() - (this.#lastUsed.get(key) ?? 0) > limit) this.#stop(key);
      if (this.#clients.size === 0) {
        clearInterval(this.#idle);
        this.#idle = undefined;
      }
    }, 60_000);
  }

  running(projectId: string | null): { server: ServerId; label: string; state: string }[] {
    return Object.entries(this.state)
      .filter(([k]) => k.startsWith(`${projectId}::`))
      .map(([k, state]) => {
        const server = k.split("::")[1] as ServerId;
        return { server, label: this.status(server)?.label ?? server, state };
      });
  }
}

export const lsp = new LspStore();
