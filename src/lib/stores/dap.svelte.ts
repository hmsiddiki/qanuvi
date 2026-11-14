/**
 * Debugging in the window (15i part 3): one debug session at a time. Starts the debugger for a launch.json
 * configuration (or a default one), follows its child sessions (js-debug runs the program in one), and keeps what the
 * "Run and debug" sidebar and the debug console show: threads, call stack, variables, watch expressions, output.
 * Breakpoints are kept per project file on this computer, whether a session runs or not.
 */
import { api, errorText, type DapEvent, type DapStatus } from "$lib/api";
import { DapClient } from "$lib/dap/client";
import {
  absPath,
  adapterFor,
  defaultConfigs,
  foldRenamed,
  forAdapter,
  launchTemplate,
  readLaunch,
  relPath,
  sourceBreakpoints,
  substitute,
  toggleLine,
  unsupported,
  type Breakpoint,
  type LaunchConfig,
  type PhpInfo,
} from "$lib/dap/protocol";
import { editor } from "./editor.svelte";
import { projects } from "./projects.svelte";
import { settings } from "./settings.svelte";

export interface Frame {
  id: number;
  name: string;
  /** Project-relative path; null for code outside the project (Node's own files). */
  path: string | null;
  sourceName: string;
  line: number;
  column: number;
  /** "subtle" or "deemphasize" for frames the configuration skips. */
  hint: string;
}

export interface Variable {
  name: string;
  value: string;
  type: string;
  ref: number;
}

export interface ConsoleLine {
  kind: "out" | "err" | "in" | "result" | "info";
  text: string;
}

interface Conn {
  id: number;
  client: DapClient;
  child: boolean;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  caps: any;
  configured: boolean;
  ended: boolean;
}

const BP_KEY = "qanuvi.breakpoints.v1";
const WATCH_KEY = "qanuvi.debug.watch.v1";
const PICK_KEY = "qanuvi.debug.pick.v1";
const MAX_LINES = 2000;

const fileKey = (projectId: string, path: string) => `${projectId}::${path}`;

function load<T>(key: string, fallback: T): T {
  try {
    const raw = typeof localStorage !== "undefined" ? localStorage.getItem(key) : null;
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}

function save(key: string, value: unknown): void {
  if (typeof localStorage !== "undefined") localStorage.setItem(key, JSON.stringify(value));
}

const INITIALIZE = {
  clientID: "qanuvi",
  clientName: "Qanuvi",
  pathFormat: "path",
  linesStartAt1: true,
  columnsStartAt1: true,
  supportsVariableType: true,
  supportsVariablePaging: false,
  supportsRunInTerminalRequest: false,
  supportsStartDebuggingRequest: true,
  supportsProgressReporting: false,
  supportsInvalidatedEvent: false,
  locale: "en",
};

class DapStore {
  statuses = $state<DapStatus[]>([]);
  /** A debugger the chosen configuration needs, not installed yet. */
  offer = $state<DapStatus | null>(null);
  installing = $state(false);
  installError = $state("");
  /** PHP and its Xdebug, as last checked. */
  php = $state<PhpInfo | null>(null);

  /** launch.json's configurations for the current project (or Qanuvi's defaults), and why it could not be read. */
  configs = $state<LaunchConfig[]>([]);
  fromFile = $state(false);
  launchError = $state("");
  picked = $state<Record<string, string>>(load(PICK_KEY, {}));

  session = $state<{
    projectId: string;
    name: string;
    adapter: string;
    status: "starting" | "running" | "stopped" | "ending";
  } | null>(null);
  lastError = $state("");
  threads = $state<{ id: number; name: string }[]>([]);
  stopped = $state<{ conn: number; threadId: number; reason: string; label: string; text: string } | null>(null);
  frames = $state<Frame[]>([]);
  frameId = $state<number | null>(null);
  scopes = $state<{ name: string; ref: number; expensive: boolean }[]>([]);
  /** Children of each variables reference, loaded when opened. */
  vars = $state<Record<number, Variable[]>>({});
  expanded = $state<Record<number, boolean>>({});
  watches = $state<Record<string, string[]>>(load(WATCH_KEY, {}));
  watchValues = $state<Record<string, { value: string; ref: number; error: boolean }>>({});
  console = $state<ConsoleLine[]>([]);
  /** Where the program is paused, for the editor's arrow and line. */
  current = $state<{ projectId: string; path: string; line: number } | null>(null);

  /** Breakpoints by project file. */
  breakpoints = $state<Record<string, Breakpoint[]>>(load(BP_KEY, {}));
  /** What the debugger answered for each file's breakpoints (lines it could not bind are unverified). */
  bound = $state<Record<string, { line: number; verified: boolean }[]>>({});
  /** Bumps when breakpoints change (the editor redraws its gutter). */
  breakpointsChanged = $state(0);

  #key: string | null = null;
  #root = "";
  #conns = new Map<number, Conn>();
  #resend = new Map<string, ReturnType<typeof setTimeout>>();
  /** Old Xdebug 2 keys PHP warned about in this session (told once). */
  #renamed = new Set<string>();

  constructor() {
    // Debuggers a previous load of this window started (a reload) are not reachable any more.
    if (typeof window !== "undefined") api.dap.stopWindow().catch(() => {});
  }

  get active(): boolean {
    return this.session !== null;
  }

  async loadStatuses(): Promise<void> {
    this.statuses = await api.dap.statuses().catch(() => this.statuses);
  }

  /** Checks PHP and the Xdebug a debug run gets (php.ini's own, or the file chosen in Settings). */
  async checkPhp(): Promise<PhpInfo> {
    const file = settings.value.languages.xdebugFile;
    const oldNote = this.php?.note;
    try {
      this.php = { ...(await api.dap.php(file)), xdebugFile: file };
    } catch (e) {
      this.php = { php: "", version: "", build: "", xdebug: "", via: "", note: errorText(e), xdebugFile: file };
    }
    // The sidebar's "no Xdebug" message goes once Settings fixed it.
    if (oldNote && this.lastError === oldNote && this.php.note !== oldNote) this.lastError = this.php.note;
    return this.php;
  }

  // Configurations -----------------------------------------------------------------------------------------------

  /** Reads launch.json for a project; without one, Qanuvi's defaults for the shown file. */
  async loadConfigs(projectId: string): Promise<void> {
    const shown = editor.active && editor.active.projectId === projectId ? editor.active.path : "";
    let file: string | null = null;
    try {
      const f = await api.files.read(projectId, ".vscode/launch.json");
      file = f.text;
    } catch {
      file = null;
    }
    if (file === null) {
      this.configs = defaultConfigs(shown);
      this.fromFile = false;
      this.launchError = "";
      return;
    }
    const r = readLaunch(file);
    this.fromFile = r.configs.length > 0;
    this.launchError = r.error;
    this.configs = r.configs.length ? r.configs : defaultConfigs(shown);
  }

  pickedConfig(projectId: string): LaunchConfig | null {
    const name = this.picked[projectId];
    return this.configs.find((c) => c.name === name) ?? this.configs[0] ?? null;
  }

  pick(projectId: string, name: string): void {
    this.picked = { ...this.picked, [projectId]: name };
    save(PICK_KEY, this.picked);
  }

  /** Opens launch.json, writing VS Code's form with Qanuvi's default first when there is none. */
  async openLaunchFile(projectId: string): Promise<void> {
    try {
      await api.files.read(projectId, ".vscode/launch.json");
    } catch {
      try {
        await api.files.create(projectId, ".vscode", true).catch(() => {});
        await api.files.create(projectId, ".vscode/launch.json", false);
        // Write, then read back: open only once the formatted text is on disk (one early read came back as one line).
        const shown = editor.active && editor.active.projectId === projectId ? editor.active.path : "";
        const text = launchTemplate(shown);
        for (let i = 0; i < 3; i++) {
          await api.files.write(projectId, ".vscode/launch.json", text, null);
          if ((await api.files.read(projectId, ".vscode/launch.json")).text === text) break;
        }
      } catch (e) {
        this.lastError = errorText(e);
        return;
      }
    }
    await editor.open(projectId, ".vscode/launch.json");
    await this.loadConfigs(projectId);
  }

  // Installing ---------------------------------------------------------------------------------------------------

  async install(id: string): Promise<boolean> {
    this.installing = true;
    this.installError = "";
    try {
      await api.dap.install(id);
      await this.loadStatuses();
      this.offer = null;
      return true;
    } catch (e) {
      this.installError = errorText(e);
      return false;
    } finally {
      this.installing = false;
    }
  }

  // Sessions -----------------------------------------------------------------------------------------------------

  #info(text: string): void {
    this.#append({ kind: "info", text });
  }

  #append(line: ConsoleLine): void {
    const next = [...this.console, line];
    this.console = next.length > MAX_LINES ? next.slice(next.length - MAX_LINES) : next;
  }

  clearConsole(): void {
    this.console = [];
  }

  /** Starts debugging a configuration in a project (stops a session that runs first). */
  async start(projectId: string, config: LaunchConfig): Promise<void> {
    this.lastError = "";
    const why = unsupported(config);
    if (why) {
      this.lastError = why;
      return;
    }
    const adapter = adapterFor(config.type)!;
    if (this.session) await this.stop();
    if (!this.statuses.length) await this.loadStatuses();
    const st = this.statuses.find((s) => s.id === adapter);
    if (!st?.installed) {
      this.offer = st ?? null;
      this.lastError = st ? `${st.label} is not installed yet.` : `Qanuvi has no debugger called ${adapter}.`;
      return;
    }
    const project = projects.list.find((p) => p.id === projectId);
    if (!project) return;
    const shown = editor.active && editor.active.projectId === projectId ? editor.active : null;
    const { config: filled, unknown } = substitute(config, {
      root: project.path,
      file: shown?.path ?? "",
      line: shown?.cursor?.line ?? 1,
      selectedText: "",
      env: {},
      home: "",
    });
    if (unknown.length) {
      this.lastError = `Qanuvi cannot fill ${unknown.join(", ")} in "${config.name}" yet; write the value in launch.json.`;
      return;
    }
    if (config.request === "launch" && config.program === "${file}" && !shown) {
      this.lastError = `"${config.name}" runs the shown file: open a file of this project first.`;
      return;
    }
    // A PHP run needs PHP with Xdebug; listening only waits for a PHP that has its own.
    const runsPhp = adapter === "php" && !!(config.program || config.runtimeArgs);
    const php = adapter === "php" ? await this.checkPhp() : null;
    if (runsPhp && php && !config.runtimeExecutable && (!php.php || !php.xdebug)) {
      this.lastError = php.note || "PHP with Xdebug was not found.";
      return;
    }

    this.#root = project.path;
    this.#resetView();
    this.console = [];
    this.#renamed = new Set();
    this.session = { projectId, name: config.name, adapter, status: "starting" };
    this.#info(`Starting "${config.name}"…`);
    if (php && runsPhp) {
      this.#info(
        `PHP ${php.version} with Xdebug ${php.xdebug}${php.via === "file" ? " (loaded for this run only)" : ""}.`,
      );
    } else if (adapter === "php") {
      this.#info(
        `Listening for Xdebug on port ${config.port ?? 9003}. Start PHP with Xdebug on (xdebug.mode=debug and ` +
          "xdebug.start_with_request=yes, or a trigger) and it stops at your breakpoints.",
      );
    }
    try {
      this.#key = await api.dap.start(projectId, adapter, (e) => this.#onAdapter(e));
    } catch (e) {
      this.lastError = errorText(e);
      this.#info(this.lastError);
      this.session = null;
      return;
    }
    this.#open(0, false, config.request, forAdapter(adapter, filled, php));
  }

  /** Opens a session on a connection: initialize, then launch or attach; breakpoints once it says it is ready. */
  #open(id: number, child: boolean, request: string, config: unknown): void {
    const key = this.#key;
    if (!key) return;
    const client = new DapClient((text) => api.dap.send(key, id, text));
    const conn: Conn = { id, client, child, caps: {}, configured: false, ended: false };
    this.#conns.set(id, conn);
    client.onEvent = (event, body) => this.#onEvent(conn, event, body);
    client.onRequest = (command, args) => this.#onRequest(conn, command, args);
    (async () => {
      try {
        conn.caps = await client.request("initialize", {
          ...INITIALIZE,
          adapterID: (config as LaunchConfig).type ?? "pwa-node",
        });
        await client.request(request, config);
        if (!child && this.session) this.session = { ...this.session, status: "running" };
      } catch (e) {
        if (conn.ended || !this.session) return;
        this.lastError = errorText(e);
        this.#info(`The debugger refused to ${request}: ${this.lastError}`);
        if (!child) await this.stop();
      }
    })();
  }

  #onAdapter(e: DapEvent): void {
    if (e.kind === "message") this.#conns.get(e.conn)?.client.receive(e.text);
    else if (e.kind === "closed") {
      const c = this.#conns.get(e.conn);
      if (c) {
        c.ended = true;
        c.client.close();
      }
      if (e.conn === 0 && this.session && this.session.status !== "ending") this.#finish();
    } else if (e.kind === "exit") {
      if (this.session && this.session.status !== "ending") this.#finish(e.code);
    }
  }

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  async #onRequest(_conn: Conn, command: string, args: any): Promise<unknown> {
    if (command === "startDebugging") {
      const key = this.#key;
      if (!key) throw new Error("No debug session.");
      const id = await api.dap.connect(key);
      this.#open(id, true, args.request ?? "launch", args.configuration ?? {});
      return {};
    }
    throw new Error(`Qanuvi does not handle "${command}" yet.`);
  }

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  #onEvent(conn: Conn, event: string, body: any): void {
    switch (event) {
      case "initialized":
        this.#configure(conn);
        break;
      case "stopped":
        this.#onStopped(conn, body);
        break;
      case "continued":
        if (this.stopped && (body.allThreadsContinued !== false || body.threadId === this.stopped.threadId)) {
          this.#running();
        }
        break;
      case "output": {
        const cat = body.category ?? "console";
        if (cat === "telemetry") break;
        let text = String(body.output ?? "").replace(/\r?\n$/, "");
        if (this.session?.adapter === "php") text = this.#foldPhp(text);
        if (text) this.#append({ kind: cat === "stderr" ? "err" : cat === "important" ? "info" : "out", text });
        break;
      }
      case "breakpoint":
        this.#onBreakpoint(body.breakpoint);
        break;
      case "exited":
        this.#info(`The program ended with exit code ${body.exitCode}.`);
        break;
      case "terminated":
        conn.ended = true;
        if (!conn.child && this.session?.status !== "ending") this.stop();
        else if (conn.child && [...this.#conns.values()].every((c) => c.child === false || c.ended)) {
          // js-debug ends its own top session after the last child; if it does not, end it here.
          setTimeout(() => this.session && this.session.status !== "ending" && this.stop(), 1500);
        }
        break;
    }
  }

  /** PHP's output without the "has been renamed" warnings; the old keys are named once per session. */
  #foldPhp(text: string): string {
    const f = foldRenamed(text);
    if (f.renamed.length) {
      const first = this.#renamed.size === 0;
      for (const k of f.renamed) this.#renamed.add(k);
      // PHP's warnings come in more than one piece: name them all once they stop coming.
      if (first) {
        const mine = this.#renamed;
        setTimeout(() => {
          if (this.#renamed !== mine) return; // a newer session started meanwhile
          this.#info(
            `php.ini still has Xdebug 2 settings that Xdebug 3 renamed (${[...mine].join(", ")}); they ` +
              "do nothing now. Qanuvi passes the Xdebug 3 settings itself. See xdebug.org/docs/upgrade_guide.",
          );
        }, 400);
      }
    }
    return f.text;
  }

  /** After `initialized`: every breakpoint of the project, then configurationDone. */
  async #configure(conn: Conn): Promise<void> {
    const s = this.session;
    if (!s) return;
    const prefix = `${s.projectId}::`;
    const files = Object.keys(this.breakpoints).filter((k) => k.startsWith(prefix));
    await Promise.all(files.map((k) => this.#sendFile(conn, s.projectId, k.slice(prefix.length))));
    if (conn.caps?.exceptionBreakpointFilters) {
      await conn.client.request("setExceptionBreakpoints", { filters: [] }).catch(() => {});
    }
    if (conn.caps?.supportsConfigurationDoneRequest) {
      await conn.client.request("configurationDone").catch(() => {});
    }
    conn.configured = true;
  }

  async #sendFile(conn: Conn, projectId: string, path: string): Promise<void> {
    const key = fileKey(projectId, path);
    const list = this.breakpoints[key] ?? [];
    try {
      const r = await conn.client.request("setBreakpoints", {
        source: { path: absPath(this.#root, path), name: path.split("/").pop() },
        breakpoints: sourceBreakpoints(list),
        lines: sourceBreakpoints(list).map((b) => b.line),
      });
      // Child sessions run the program: their answer is the one that counts.
      if (conn.child || !this.bound[key]) {
        const sent = sourceBreakpoints(list);
        const got = (r.breakpoints ?? []) as { verified?: boolean; line?: number }[];
        this.bound = {
          ...this.bound,
          [key]: sent.map((b, i) => ({ line: b.line, verified: got[i]?.verified !== false })),
        };
        this.breakpointsChanged++;
      }
    } catch {
      // A session that ended meanwhile.
    }
  }

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  #onBreakpoint(bp: any): void {
    if (!bp?.source?.path || typeof bp.line !== "number" || !this.session) return;
    const path = relPath(this.#root, bp.source.path);
    if (!path) return;
    const key = fileKey(this.session.projectId, path);
    const list = this.bound[key];
    if (!list) return;
    this.bound = {
      ...this.bound,
      [key]: list.map((b) => (b.line === bp.line ? { ...b, verified: bp.verified !== false } : b)),
    };
    this.breakpointsChanged++;
  }

  #resetView(): void {
    this.threads = [];
    this.stopped = null;
    this.frames = [];
    this.frameId = null;
    this.scopes = [];
    this.vars = {};
    this.expanded = {};
    this.watchValues = {};
    this.current = null;
    this.bound = {};
    this.breakpointsChanged++;
  }

  #running(): void {
    this.stopped = null;
    this.frames = [];
    this.frameId = null;
    this.scopes = [];
    this.vars = {};
    this.expanded = {};
    this.current = null;
    if (this.session && this.session.status !== "ending") this.session = { ...this.session, status: "running" };
  }

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  async #onStopped(conn: Conn, body: any): Promise<void> {
    if (!this.session) return;
    const c = conn.client;
    let threads: { id: number; name: string }[] = [];
    try {
      threads = ((await c.request("threads")).threads ?? []) as { id: number; name: string }[];
    } catch {
      // Keep going without the list.
    }
    const threadId: number = body.threadId ?? threads[0]?.id ?? 0;
    const reason = String(body.reason ?? "pause");
    this.threads = threads;
    const label = String(body.description ?? "") || `Paused on ${reason}`;
    const text = String(body.text ?? "");
    this.stopped = { conn: conn.id, threadId, reason, label, text: text === label ? "" : text };
    this.session = { ...this.session, status: "stopped" };
    this.vars = {};
    this.expanded = {};
    try {
      const st = await c.request("stackTrace", { threadId, startFrame: 0, levels: 50 });
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      this.frames = (st.stackFrames ?? []).map((f: any) => ({
        id: f.id,
        name: f.name,
        path: f.source?.path ? relPath(this.#root, f.source.path) : null,
        sourceName: f.source?.name ?? "",
        line: f.line ?? 0,
        column: f.column ?? 1,
        hint: f.presentationHint ?? f.source?.presentationHint ?? "",
      }));
    } catch (e) {
      this.frames = [];
      this.#info(`No call stack: ${errorText(e)}`);
    }
    const first = this.frames.find((f) => f.path && f.hint !== "deemphasize" && f.hint !== "subtle") ?? this.frames[0];
    if (first) await this.selectFrame(first);
  }

  /** Shows a frame: its file at its line, its scopes (the first opened), the watch values in it. */
  async selectFrame(f: Frame): Promise<void> {
    const s = this.session;
    const conn = this.#focus();
    if (!s || !conn) return;
    this.frameId = f.id;
    this.current = f.path ? { projectId: s.projectId, path: f.path, line: f.line } : null;
    if (f.path) editor.open(s.projectId, f.path, f.line, f.column).catch(() => {});
    try {
      const r = await conn.client.request("scopes", { frameId: f.id });
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      this.scopes = (r.scopes ?? []).map((x: any) => ({
        name: x.name,
        ref: x.variablesReference,
        expensive: !!x.expensive,
      }));
    } catch {
      this.scopes = [];
    }
    const open = this.scopes.find((x) => !x.expensive);
    if (open) await this.toggle(open.ref, true);
    await this.refreshWatches();
  }

  /** The connection commands go to: the one that stopped, else the newest child, else the top one. */
  #focus(): Conn | null {
    if (this.stopped) {
      const c = this.#conns.get(this.stopped.conn);
      if (c && !c.ended) return c;
    }
    const live = [...this.#conns.values()].filter((c) => !c.ended);
    return live.filter((c) => c.child).pop() ?? live[0] ?? null;
  }

  /** Opens or closes a variable (or scope); its children load the first time. */
  async toggle(ref: number, open?: boolean): Promise<void> {
    const next = open ?? !this.expanded[ref];
    this.expanded = { ...this.expanded, [ref]: next };
    if (!next || this.vars[ref]) return;
    const conn = this.#focus();
    if (!conn) return;
    try {
      const r = await conn.client.request("variables", { variablesReference: ref });
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const list: Variable[] = (r.variables ?? []).map((v: any) => ({
        name: v.name,
        value: v.value ?? "",
        type: v.type ?? "",
        ref: v.variablesReference ?? 0,
      }));
      this.vars = { ...this.vars, [ref]: list };
    } catch (e) {
      this.vars = { ...this.vars, [ref]: [{ name: "", value: errorText(e), type: "", ref: 0 }] };
    }
  }

  // Controls -----------------------------------------------------------------------------------------------------

  async #step(command: "continue" | "next" | "stepIn" | "stepOut"): Promise<void> {
    const conn = this.#focus();
    const t = this.stopped?.threadId;
    if (!conn || t === undefined) return;
    this.#running();
    await conn.client.request(command, { threadId: t }).catch((e) => (this.lastError = errorText(e)));
  }

  continue = () => this.#step("continue");
  stepOver = () => this.#step("next");
  stepIn = () => this.#step("stepIn");
  stepOut = () => this.#step("stepOut");

  async pause(): Promise<void> {
    const conn = this.#focus();
    if (!conn || this.stopped) return;
    const threadId = this.threads[0]?.id ?? (await conn.client.request("threads").catch(() => ({})))?.threads?.[0]?.id;
    await conn.client.request("pause", { threadId: threadId ?? 0 }).catch((e) => (this.lastError = errorText(e)));
  }

  async restart(): Promise<void> {
    const s = this.session;
    if (!s) return;
    const config = this.configs.find((c) => c.name === s.name);
    await this.stop();
    if (config) await this.start(s.projectId, config);
  }

  /** Ends the session: the debuggee too (disconnect with terminateDebuggee), then the debugger itself. */
  async stop(): Promise<void> {
    const s = this.session;
    if (!s || s.status === "ending") return;
    this.session = { ...s, status: "ending" };
    const conns = [...this.#conns.values()].filter((c) => !c.ended).sort((a, b) => Number(a.child) - Number(b.child));
    for (const c of conns.reverse()) {
      await c.client.request("disconnect", { terminateDebuggee: true }, 1500).catch(() => {});
    }
    this.#finish();
  }

  #finish(code?: number | null): void {
    const key = this.#key;
    this.#key = null;
    for (const c of this.#conns.values()) c.client.close();
    this.#conns.clear();
    if (key) api.dap.stop(key).catch(() => {});
    this.#resetView();
    if (this.session) {
      this.#info(code ? `The debugger ended (code ${code}).` : "Debugging ended.");
    }
    this.session = null;
  }

  // Console and watch --------------------------------------------------------------------------------------------

  /** Evaluates an expression in the selected frame (the debug console's input). */
  async evaluate(expression: string): Promise<void> {
    const text = expression.trim();
    if (!text) return;
    this.#append({ kind: "in", text });
    const conn = this.#focus();
    if (!conn) {
      this.#info("Start debugging first.");
      return;
    }
    try {
      const r = await conn.client.request("evaluate", {
        expression: text,
        context: "repl",
        ...(this.frameId !== null ? { frameId: this.frameId } : {}),
      });
      this.#append({ kind: "result", text: String(r.result ?? "") });
    } catch (e) {
      this.#append({ kind: "err", text: errorText(e) });
    }
  }

  watchList(projectId: string): string[] {
    return this.watches[projectId] ?? [];
  }

  addWatch(projectId: string, expression: string): void {
    const e = expression.trim();
    if (!e || this.watchList(projectId).includes(e)) return;
    this.watches = { ...this.watches, [projectId]: [...this.watchList(projectId), e] };
    save(WATCH_KEY, this.watches);
    this.refreshWatches();
  }

  removeWatch(projectId: string, expression: string): void {
    this.watches = { ...this.watches, [projectId]: this.watchList(projectId).filter((x) => x !== expression) };
    save(WATCH_KEY, this.watches);
  }

  async refreshWatches(): Promise<void> {
    const s = this.session;
    const conn = this.#focus();
    if (!s || !conn || this.frameId === null) return;
    const out: Record<string, { value: string; ref: number; error: boolean }> = {};
    for (const e of this.watchList(s.projectId)) {
      try {
        const r = await conn.client.request("evaluate", { expression: e, context: "watch", frameId: this.frameId });
        out[e] = { value: String(r.result ?? ""), ref: r.variablesReference ?? 0, error: false };
      } catch (err) {
        out[e] = { value: errorText(err), ref: 0, error: true };
      }
    }
    this.watchValues = out;
  }

  // Breakpoints --------------------------------------------------------------------------------------------------

  breakpointsFor(projectId: string, path: string): Breakpoint[] {
    return this.breakpoints[fileKey(projectId, path)] ?? [];
  }

  /** Each breakpoint with whether the running debugger could bind it (true while none runs). */
  shownBreakpoints(projectId: string, path: string): { line: number; verified: boolean; enabled: boolean }[] {
    const key = fileKey(projectId, path);
    const bound = this.session?.projectId === projectId ? this.bound[key] : undefined;
    return this.breakpointsFor(projectId, path).map((b) => ({
      line: b.line,
      enabled: b.enabled,
      verified: !bound || bound.find((x) => x.line === b.line)?.verified !== false,
    }));
  }

  get allBreakpoints(): { projectId: string; path: string; bp: Breakpoint }[] {
    const out: { projectId: string; path: string; bp: Breakpoint }[] = [];
    for (const [k, list] of Object.entries(this.breakpoints)) {
      const i = k.indexOf("::");
      for (const bp of list) out.push({ projectId: k.slice(0, i), path: k.slice(i + 2), bp });
    }
    return out;
  }

  #set(projectId: string, path: string, list: Breakpoint[]): void {
    const key = fileKey(projectId, path);
    const next = { ...this.breakpoints };
    if (list.length) next[key] = list;
    else delete next[key];
    this.breakpoints = next;
    save(BP_KEY, next);
    this.breakpointsChanged++;
    // A running session hears about it (shortly, so typing that moves lines sends once).
    if (this.session?.projectId === projectId) {
      clearTimeout(this.#resend.get(key));
      this.#resend.set(
        key,
        setTimeout(() => {
          this.#resend.delete(key);
          for (const c of this.#conns.values()) if (!c.ended && c.configured) this.#sendFile(c, projectId, path);
        }, 150),
      );
    }
  }

  toggleBreakpoint(projectId: string, path: string, line: number): void {
    this.#set(projectId, path, toggleLine(this.breakpointsFor(projectId, path), line));
  }

  setEnabled(projectId: string, path: string, line: number, enabled: boolean): void {
    this.#set(
      projectId,
      path,
      this.breakpointsFor(projectId, path).map((b) => (b.line === line ? { ...b, enabled } : b)),
    );
  }

  setCondition(projectId: string, path: string, line: number, condition: string): void {
    this.#set(
      projectId,
      path,
      this.breakpointsFor(projectId, path).map((b) =>
        b.line === line ? { ...b, condition: condition.trim() || undefined } : b,
      ),
    );
  }

  /** The editor moved breakpoints with an edit (lines added or removed above them). */
  moveBreakpoints(projectId: string, path: string, lines: number[]): void {
    const old = this.breakpointsFor(projectId, path);
    if (old.length !== lines.length || old.every((b, i) => b.line === lines[i])) return;
    const seen = new Set<number>();
    const list: Breakpoint[] = [];
    old.forEach((b, i) => {
      if (seen.has(lines[i])) return;
      seen.add(lines[i]);
      list.push({ ...b, line: lines[i] });
    });
    this.#set(projectId, path, list);
  }

  clearBreakpoints(projectId: string): void {
    for (const k of Object.keys(this.breakpoints)) {
      if (k.startsWith(`${projectId}::`)) this.#set(projectId, k.slice(projectId.length + 2), []);
    }
  }
}

export const dap = new DapStore();
