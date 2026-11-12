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

export type BottomView = "terminal" | "problems" | "references";

const clientKey = (projectId: string, server: ServerId) => `${projectId}::${server}`;

class LspStore {
  statuses = $state<LspStatus[]>([]);
  /** Problems by file (project id and path). */
  problems = $state<Record<string, Problem[]>>({});
  /** Bumps when any file's problems change (the editor redraws its underlines). */
  problemsChanged = $state(0);
  /** A server to offer for the shown file: install it, not now, never. */
  offer = $state<{ server: ServerId; status: LspStatus } | null>(null);
  installing = $state<string | null>(null);
  installError = $state("");
  /** Server state per project and language, for the status bar. */
  state = $state<Record<string, "starting" | "ready" | "stopped" | "failed">>({});
  lastError = $state("");
  references = $state<{ title: string; places: Place[] } | null>(null);
  bottom = $state<BottomView>("terminal");
  /** Applies a WorkspaceEdit in a project (set by lsp/actions.ts, which knows the editor). */
  editApplier: (projectId: string, edit: unknown) => Promise<boolean> = async () => false;

  #clients = new Map<string, LspClient>();
  #lastUsed = new Map<string, number>();
  /** Which tab key has which document open, to close the ones whose tabs closed. */
  #docs = new Map<string, { client: string; uri: string }>();
  #timers = new Map<string, ReturnType<typeof setTimeout>>();
  #notNow = new Set<string>();
  #statusLoad: Promise<void> | null = null;
  #statusAt = 0;
  #idle: ReturnType<typeof setInterval> | undefined;

  constructor() {
    // Servers a previous load of this window started (a reload) are not reachable any more.
    if (typeof window !== "undefined") api.lsp.stopWindow().catch(() => {});
  }

  loadStatuses(force = false): Promise<void> {
    if (!this.#statusLoad || force) {
      this.#statusAt = Date.now();
      this.#statusLoad = api.lsp
        .statuses()
        .then((s) => void (this.statuses = s))
        .catch(() => void (this.statuses = []));
    }
    return this.#statusLoad;
  }

  status(server: ServerId): LspStatus | undefined {
    return this.statuses.find((s) => s.id === server);
  }

  enabled(server: ServerId): boolean {
    return !settings.value.languages.off.includes(server);
  }

  setEnabled(server: ServerId, on: boolean): void {
    const l = settings.value.languages;
    const off = on ? l.off.filter((x) => x !== server) : [...new Set([...l.off, server])];
    settings.set("languages", { ...l, off });
    if (!on) this.stopServer(server);
  }

  #root(projectId: string): string | null {
    const p = projects.list.find((x) => x.id === projectId);
    // Projects on a server (Milestone 16) get no language servers from this computer.
    return p && !p.master ? p.path : null;
  }

  /** The client for a file, started if needed; null when there is none (no server, off, not installed). */
  async clientFor(doc: Doc): Promise<{ client: LspClient; uri: string } | null> {
    const server = serverFor(doc.path);
    const root = this.#root(doc.projectId);
    if (!server || !root || !this.enabled(server)) return null;
    const key = clientKey(doc.projectId, server);
    let client = this.#clients.get(key);
    if (!client) {
      await this.loadStatuses();
      let st = this.status(server);
      // Installed meanwhile from outside this window (another window, npm or rustup in a terminal): look again,
      // at most every 30 s, since the check starts programs (rust-analyzer --version).
      if (!st?.found && Date.now() - this.#statusAt > 30_000) {
        await this.loadStatuses(true);
        st = this.status(server);
      }
      if (!st?.found) {
        const l = settings.value.languages;
        if (st && !l.never.includes(server) && !this.#notNow.has(server)) this.offer = { server, status: st };
        return null;
      }
      client = new LspClient(doc.projectId, server, root, (why) => {
        if (this.#clients.get(key) !== client) return;
        this.#clients.delete(key);
        this.#forget(key);
        this.#clearProblems(doc.projectId, server);
        this.state = { ...this.state, [key]: "stopped" };
        if (why) this.lastError = `${st.label} stopped: ${why}`;
      });
      this.#clients.set(key, client);
      this.state = { ...this.state, [key]: "starting" };
      const c = client;
      c.applyEdit = (edit) => this.editApplier(doc.projectId, edit);
      c.on("textDocument/publishDiagnostics", (p) => this.#diagnostics(doc.projectId, root, p));
      c.ready.then(
        () => this.#clients.get(key) === c && (this.state = { ...this.state, [key]: "ready" }),
        (e) => {
          this.#clients.delete(key);
          this.state = { ...this.state, [key]: "failed" };
          this.lastError = `${st.label}: ${errorText(e)}`;
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
    this.#docs.set(doc.key, { client: clientKey(doc.projectId, c.client.server), uri: c.uri });
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
  async install(server: ServerId): Promise<boolean> {
    this.installing = server;
    this.installError = "";
    try {
      const st = await api.lsp.install(server);
      this.statuses = this.statuses.map((s) => (s.id === st.id ? st : s));
      if (this.offer?.server === server) this.offer = null;
      return true;
    } catch (e) {
      this.installError = errorText(e);
      return false;
    } finally {
      this.installing = null;
    }
  }

  // Stopping ------------------------------------------------------------------------------------------------------

  #stop(key: string) {
    const c = this.#clients.get(key);
    if (!c) return;
    this.#clients.delete(key);
    this.#forget(key);
    this.#clearProblems(c.projectId, c.server);
    const s = { ...this.state };
    delete s[key];
    this.state = s;
    c.stop();
  }

  stopServer(server: ServerId): void {
    for (const [key, c] of this.#clients) if (c.server === server) this.#stop(key);
  }

  stopProject(projectId: string): void {
    for (const [key, c] of this.#clients) if (c.projectId === projectId) this.#stop(key);
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
