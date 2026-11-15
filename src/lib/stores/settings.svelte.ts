/**
 * User preferences (24z3 Part A3): the Settings page is the shown machine's, kept in that machine's database
 * (settings_get / settings_set, one value per top-level key). The window keeps its own things (its list of masters,
 * its sign-in) and a copy of This PC's settings in local storage, so it starts at once with them.
 */
import { invoke } from "@tauri-apps/api/core";
import { callMaster, shownMachine } from "$lib/remote";
import { DEFAULT_TEAM, parseTeam } from "$lib/core/decider";
import { PROVIDERS, providerById } from "$lib/agents/providers";
import { own } from "$lib/agents/ownModels.svelte";

const KEY = "qanuvi.settings.v1";

/** Settings that stay with the window, never kept by a machine (settings.rs refuses them too). */
export const WINDOW_KEYS = ["masters", "account"] as const;
const isWindowKey = (k: string) => (WINDOW_KEYS as readonly string[]).includes(k);

type Values = Record<string, unknown>;

/** The keys of a settings object a machine keeps. */
export function machinePart(v: Values): Values {
  return Object.fromEntries(Object.entries(v).filter(([k]) => !isWindowKey(k)));
}

/** The keys whose value differs between two settings objects (a machine's keys only). */
export function changedKeys(before: Values, after: Values): Values {
  const out: Values = {};
  for (const k of new Set([...Object.keys(before), ...Object.keys(after)])) {
    if (isWindowKey(k)) continue;
    if (JSON.stringify(before[k]) !== JSON.stringify(after[k])) out[k] = after[k] ?? null;
  }
  return out;
}

function readLocal(): Values {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? "null");
    return v && typeof v === "object" && !Array.isArray(v) ? (v as Values) : {};
  } catch {
    return {};
  }
}

/** Per-agent choices; anything left empty falls back to the agent's own default. */
export interface AgentSettings {
  model: string;
  mode: string;
  effort: string;
  /** Address the agent talks to instead of its own (a proxy). Empty: the agent's own setting. */
  baseUrl: string;
  /** Give the agent Qanuvi's code-index tools (find a definition, outline a file, read some lines). */
  indexTools: boolean;
  /** Give the agent Qanuvi's test browser (open a page, read it, click, type, errors; screenshots on request). */
  browserTools: boolean;
}

export interface Settings {
  /** Editor tabs belong to a project and are swapped when the project changes. */
  tabsFollowProject: boolean;
  defaultShell: string;
  explorerWidth: number;
  explorerVisible: boolean;
  /** AI features (agent pane, sessions, plan runs). Off: Qanuvi is a plain code editor. */
  ai: boolean;
  /** The agent new sessions use. */
  agent: string;
  agents: Record<string, AgentSettings>;
  /** Monthly limit for all agent work, in US dollars; 0 means none. */
  monthlyBudgetUsd: number;
  /** Per project: job type -> model to start steps of that kind on. */
  rules: Record<string, Record<string, string>>;
  /** Give agents the relevant part of the user's memory with their work (Milestone 17). */
  memoryContext: boolean;
  /** At most this many characters of memory per request (about a quarter as many tokens). */
  memoryBudget: number;
  /** Servers running Qanuvi without a window, reached over SSH (Milestone 16). */
  masters: MasterSettings[];
  /** Where the encrypted copy of the memory is kept (Milestone 18). */
  memoryDisk: MemoryDiskSettings;
  /** Letting this computer be reached through a relay (Milestone 20). */
  remoteAccess: { enabled: boolean; relay: string; name: string };
  /** Signed in to the central Qanuvi account (Milestone 19); the sign-in itself is kept in the credential store. */
  account: { signedIn: boolean };
  /** Qanuvi's own agent (Milestone 24): where models are reached and the ones to use, best first. */
  /** local: false = free models online only (OpenRouter, OmniRoute); nothing is installed, loaded or started here. */
  /** omniAssist: Qanuvi runs OmniRoute for the user (connections, sign-ins, combo); off for those who use its panel. */
  brain: { providers: ModelProvider[]; models: OwnModel[]; local: boolean; omniAssist: boolean };
  /** How the team works: who decides, who works (24b). */
  team: import("$lib/core/decider").TeamSettings;
  /** How long "Allow for N minutes" lasts on Qanuvi's own agent's approval cards (1–240). */
  approveMinutes: number;
  /** How Qanuvi looks (15h): the editor's font size (px), the size of the whole interface (%), the minimap (15i). */
  appearance: { editorFontSize: number; uiScale: number; minimap: boolean };
  /** Start with Qanuvi: commands started hidden when Qanuvi starts (a Headroom proxy, for example). */
  startup: StartupItem[];
  /** Updates (1b): off, a newer version waits for the user's click in the status bar before it downloads. */
  updates: { autoDownload: boolean; claudeAuto?: boolean };
  /**
   * Language servers (15i): servers turned off, servers not to offer again, minutes unused before one stops, whether
   * Ctrl+S formats the file through its server first, and an Xdebug file loaded for PHP debug runs only ("" for none).
   */
  languages: { off: string[]; never: string[]; idleMinutes: number; formatOnSave: boolean; xdebugFile: string };
  /**
   * Chat handover (24s): past `limit` tokens of context (0 = never) a chat goes on in a new session that starts from
   * a handover note. ask: a bar asks first; auto: it happens by itself; off: never.
   */
  chatHandover: { when: "ask" | "auto" | "off"; limit: number };
  /** Chat clean-up (24t): chats not opened for this many days are deleted at start-up (0 = off). */
  chatCleanupDays: 0 | 30 | 60 | 90;
  /**
   * Desktop notifications of new Inbox items (24u part 4, session B): on or off, which items, several together told as
   * one, and quiet hours (minutes after midnight, local time). The Inbox keeps every item whatever is shown.
   */
  notifications: NotifySettings;
}

export interface NotifySettings {
  desktop: boolean;
  what: "questions" | "problems" | "all";
  bundle: boolean;
  quiet: boolean;
  quietFrom: number;
  quietTo: number;
}

export const DEFAULT_NOTIFICATIONS: NotifySettings = {
  desktop: true,
  what: "problems",
  bundle: true,
  quiet: false,
  quietFrom: 22 * 60,
  quietTo: 7 * 60,
};

/** Reads stored notification settings; anything odd falls back to its default. */
export function parseNotifications(raw: unknown): NotifySettings {
  const r = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
  const d = DEFAULT_NOTIFICATIONS;
  const minute = (v: unknown, def: number) =>
    typeof v === "number" && Number.isFinite(v) ? Math.min(1439, Math.max(0, Math.round(v))) : def;
  return {
    desktop: typeof r.desktop === "boolean" ? r.desktop : d.desktop,
    what: r.what === "questions" || r.what === "all" ? r.what : "problems",
    bundle: typeof r.bundle === "boolean" ? r.bundle : d.bundle,
    quiet: r.quiet === true,
    quietFrom: minute(r.quietFrom, d.quietFrom),
    quietTo: minute(r.quietTo, d.quietTo),
  };
}

/** Default context size at which a chat offers a handover (24s). */
export const CHAT_HANDOVER_LIMIT = 150_000;

/** A command Qanuvi starts with itself (hidden), and the address that answers once it runs. */
export interface StartupItem {
  id: string;
  name: string;
  command: string;
  /** host:port, for example 127.0.0.1:8787; empty: no check. */
  waitFor: string;
  enabled: boolean;
}

export type ModelProviderKind = "ollama" | "openai" | "openrouter";

/** A place models are reached. Its key (if any) is kept in the credential store, never here. */
export interface ModelProvider {
  id: string;
  name: string;
  kind: ModelProviderKind;
  url: string;
  /** A server on this computer: the command that starts it (llama.cpp, LM Studio, vLLM…), run when it does not answer. */
  start?: string;
}

export interface OwnModel {
  /** provider id / model name */
  id: string;
  label: string;
  local: boolean;
  free: boolean;
  context: number;
}

export const DEFAULT_MODEL_PROVIDERS: ModelProvider[] = [
  { id: "ollama", name: "Ollama (this computer)", kind: "ollama", url: "http://127.0.0.1:11434" },
  { id: "openrouter", name: "OpenRouter", kind: "openrouter", url: "https://openrouter.ai/api/v1" },
  { id: "omniroute", name: "OmniRoute", kind: "openai", url: "http://localhost:20128/v1" },
];

export interface MemoryDiskSettings {
  /** "off": this machine only; "folder": a folder (a cloud drive's folder works); "server": a folder on a server. */
  mode: "off" | "folder" | "server";
  folder: string;
  /** A server from Servers (its SSH address and key are used). */
  server: string;
  serverDir: string;
  /** Sync at start-up, shortly after changes and every 10 minutes. */
  auto: boolean;
}

export interface MasterSettings {
  id: string;
  /** Shown exactly as typed; empty until the machine says its own computer name (filled in on connect). */
  name: string;
  /** Its colour in the switcher and the status bar (machineNames.ts); picked on the first connect. */
  color?: string;
  /** The machine's lasting id, from its hello: adding the same computer twice is refused. */
  machineId?: string;
  /** SSH address: user@host, a host name or an alias from the SSH config; "local" for the master on this PC. */
  target: string;
  /** The master program on the server. */
  program: string;
  /** SSH key file for this server (empty: SSH's own defaults and config). */
  identity: string;
  /** Connect when Qanuvi starts. */
  autoConnect: boolean;
  /** Reached through the relay (Milestone 20) instead of SSH: the relay and the pairing (its key is in the credential store). */
  relay?: { url: string; masterId: string; pairing: string };
}

const DEFAULTS: Settings = {
  tabsFollowProject: true,
  defaultShell: "",
  explorerWidth: 240,
  explorerVisible: true,
  ai: true,
  agent: PROVIDERS[0].id,
  agents: {},
  monthlyBudgetUsd: 0,
  rules: {},
  masters: [],
  memoryContext: true,
  memoryBudget: 1500,
  memoryDisk: { mode: "off", folder: "", server: "", serverDir: "~/.qanuvi-memory", auto: true },
  account: { signedIn: false },
  remoteAccess: { enabled: false, relay: "", name: "" },
  brain: { providers: DEFAULT_MODEL_PROVIDERS, models: [], local: true, omniAssist: true },
  approveMinutes: 10,
  appearance: { editorFontSize: 13, uiScale: 100, minimap: false },
  updates: { autoDownload: false },
  languages: { off: [], never: [], idleMinutes: 10, formatOnSave: false, xdebugFile: "" },
  chatHandover: { when: "ask", limit: CHAT_HANDOVER_LIMIT },
  chatCleanupDays: 60,
  notifications: DEFAULT_NOTIFICATIONS,
  startup: [],
  team: DEFAULT_TEAM,
};

function cleanAgent(providerId: string, raw: unknown): AgentSettings {
  const p = providerById(providerId);
  const r = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
  const str = (v: unknown) => (typeof v === "string" ? v.trim() : "");
  const model = str(r.model);
  const mode = str(r.mode);
  const effort = str(r.effort);
  return {
    model: model && !/\s/.test(model) ? model.slice(0, 160) : p.defaultModel,
    mode: p.modes.some((m) => m.id === mode) ? mode : p.defaultMode,
    effort: p.efforts.includes(effort) ? effort : p.defaultEffort,
    baseUrl: str(r.baseUrl).slice(0, 300),
    indexTools: typeof r.indexTools === "boolean" ? r.indexTools : true,
    browserTools: typeof r.browserTools === "boolean" ? r.browserTools : true,
  };
}

export function parseSettings(json: string | null): Settings {
  let raw: Record<string, unknown> = {};
  try {
    const parsed = json ? JSON.parse(json) : null;
    if (parsed && typeof parsed === "object") raw = parsed as Record<string, unknown>;
  } catch {
    raw = {};
  }
  const s: Settings = { ...DEFAULTS, agents: {}, rules: {}, masters: [] };
  const disk = raw.memoryDisk && typeof raw.memoryDisk === "object" ? (raw.memoryDisk as Record<string, unknown>) : {};
  const text = (v: unknown, max: number) => (typeof v === "string" ? v.trim().slice(0, max) : "");
  s.memoryDisk = {
    mode: disk.mode === "folder" || disk.mode === "server" ? disk.mode : "off",
    folder: text(disk.folder, 500),
    server: text(disk.server, 60),
    serverDir: text(disk.serverDir, 300) || "~/.qanuvi-memory",
    auto: disk.auto !== false,
  };
  const acct = raw.account && typeof raw.account === "object" ? (raw.account as Record<string, unknown>) : {};
  s.account = { signedIn: acct.signedIn === true };
  const ra =
    raw.remoteAccess && typeof raw.remoteAccess === "object" ? (raw.remoteAccess as Record<string, unknown>) : {};
  s.remoteAccess = { enabled: ra.enabled === true, relay: text(ra.relay, 300), name: text(ra.name, 60) };
  s.brain = parseBrain(raw.brain);
  s.team = parseTeam(raw.team);
  s.approveMinutes =
    typeof raw.approveMinutes === "number" && Number.isFinite(raw.approveMinutes)
      ? Math.min(240, Math.max(1, Math.round(raw.approveMinutes)))
      : 10;
  {
    const ap = raw.appearance && typeof raw.appearance === "object" ? (raw.appearance as Record<string, unknown>) : {};
    const n = (v: unknown, d: number, lo: number, hi: number) =>
      typeof v === "number" && Number.isFinite(v) ? Math.min(hi, Math.max(lo, Math.round(v))) : d;
    s.appearance = {
      editorFontSize: n(ap.editorFontSize, 13, 10, 24),
      uiScale: n(ap.uiScale, 100, 80, 150),
      minimap: ap.minimap === true,
    };
  }
  {
    const up = raw.updates && typeof raw.updates === "object" ? (raw.updates as Record<string, unknown>) : {};
    s.updates = { autoDownload: up.autoDownload === true, claudeAuto: up.claudeAuto === true };
  }
  {
    const lg = raw.languages && typeof raw.languages === "object" ? (raw.languages as Record<string, unknown>) : {};
    const ids = (v: unknown) =>
      Array.isArray(v) ? [...new Set(v.filter((x): x is string => typeof x === "string" && x.length < 40))] : [];
    const idle = typeof lg.idleMinutes === "number" && Number.isFinite(lg.idleMinutes) ? lg.idleMinutes : 10;
    s.languages = {
      off: ids(lg.off),
      never: ids(lg.never),
      idleMinutes: Math.min(240, Math.max(1, Math.round(idle))),
      formatOnSave: lg.formatOnSave === true,
      xdebugFile: typeof lg.xdebugFile === "string" ? lg.xdebugFile.trim().slice(0, 1000) : "",
    };
  }
  {
    const h =
      raw.chatHandover && typeof raw.chatHandover === "object" ? (raw.chatHandover as Record<string, unknown>) : {};
    const limit = typeof h.limit === "number" && Number.isFinite(h.limit) ? h.limit : CHAT_HANDOVER_LIMIT;
    s.chatHandover = {
      when: h.when === "auto" || h.when === "off" ? h.when : "ask",
      limit: Math.min(1_000_000, Math.max(0, Math.round(limit))),
    };
  }
  s.chatCleanupDays = ([0, 30, 60, 90] as const).find((d) => d === raw.chatCleanupDays) ?? 60;
  s.notifications = parseNotifications(raw.notifications);
  if (Array.isArray(raw.startup))
    for (const it of raw.startup as Record<string, unknown>[]) {
      const str = (v: unknown) => (typeof v === "string" ? v.trim() : "");
      if (!it || typeof it !== "object" || !str(it.id) || !str(it.command)) continue;
      s.startup.push({
        id: str(it.id),
        name: str(it.name) || str(it.command).split(" ")[0],
        command: str(it.command),
        waitFor: str(it.waitFor),
        enabled: it.enabled !== false,
      });
    }
  if (Array.isArray(raw.masters))
    for (const m of raw.masters as Record<string, unknown>[]) {
      const str = (v: unknown) => (typeof v === "string" ? v.trim() : "");
      if (!m || typeof m !== "object" || !str(m.id) || !str(m.target)) continue;
      s.masters.push({
        id: str(m.id).slice(0, 60),
        // Empty until the machine says its computer name (masters.svelte.ts fills it in on connect).
        name: str(m.name).slice(0, 60),
        ...(/^#[0-9a-f]{6}$/i.test(str(m.color)) ? { color: str(m.color) } : {}),
        ...(str(m.machineId) ? { machineId: str(m.machineId).slice(0, 64) } : {}),
        target: str(m.target).slice(0, 200),
        program: str(m.program).slice(0, 300) || "qanuvi-master",
        identity: str(m.identity).slice(0, 400),
        autoConnect: m.autoConnect === true,
        ...(m.relay && typeof m.relay === "object"
          ? (() => {
              const r = m.relay as Record<string, unknown>;
              return {
                relay: {
                  url: str(r.url).slice(0, 300),
                  masterId: str(r.masterId).slice(0, 64),
                  pairing: str(r.pairing).slice(0, 64),
                },
              };
            })()
          : {}),
      });
    }
  if (typeof raw.monthlyBudgetUsd === "number" && Number.isFinite(raw.monthlyBudgetUsd))
    s.monthlyBudgetUsd = Math.min(100000, Math.max(0, raw.monthlyBudgetUsd));
  if (raw.rules && typeof raw.rules === "object")
    for (const [project, r] of Object.entries(raw.rules as Record<string, unknown>)) {
      if (!r || typeof r !== "object") continue;
      const clean: Record<string, string> = {};
      for (const [type, model] of Object.entries(r as Record<string, unknown>))
        if (typeof model === "string" && model.trim() && !/\s/.test(model.trim())) clean[type] = model.trim();
      if (Object.keys(clean).length) s.rules[project] = clean;
    }
  if (typeof raw.tabsFollowProject === "boolean") s.tabsFollowProject = raw.tabsFollowProject;
  if (typeof raw.defaultShell === "string") s.defaultShell = raw.defaultShell;
  if (typeof raw.explorerWidth === "number" && Number.isFinite(raw.explorerWidth))
    s.explorerWidth = Math.min(520, Math.max(160, Math.round(raw.explorerWidth)));
  if (typeof raw.explorerVisible === "boolean") s.explorerVisible = raw.explorerVisible;
  if (typeof raw.ai === "boolean") s.ai = raw.ai;
  if (typeof raw.memoryContext === "boolean") s.memoryContext = raw.memoryContext;
  if (typeof raw.memoryBudget === "number" && Number.isFinite(raw.memoryBudget))
    s.memoryBudget = Math.min(8000, Math.max(300, Math.round(raw.memoryBudget)));
  if (typeof raw.agent === "string" && PROVIDERS.some((p) => p.id === raw.agent)) s.agent = raw.agent;

  const agents = raw.agents && typeof raw.agents === "object" ? (raw.agents as Record<string, unknown>) : {};
  for (const p of PROVIDERS) {
    // Settings saved before agents had their own section are carried over to the first agent.
    const legacy =
      p.id === PROVIDERS[0].id && !(p.id in agents)
        ? { model: raw.defaultModel, mode: raw.defaultMode, baseUrl: raw.agentBaseUrl }
        : undefined;
    s.agents[p.id] = cleanAgent(p.id, agents[p.id] ?? legacy);
  }
  return s;
}

/** Qanuvi's own agent's providers and models, checked: ids of letters, digits and dashes, http(s) addresses, models
 * that belong to a provider. */
export function parseBrain(raw: unknown): Settings["brain"] {
  const b = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
  const text = (v: unknown, max: number) => (typeof v === "string" ? v.trim().slice(0, max) : "");
  const providers: ModelProvider[] = [];
  const list = Array.isArray(b.providers) ? (b.providers as Record<string, unknown>[]) : DEFAULT_MODEL_PROVIDERS;
  for (const p of list) {
    if (!p || typeof p !== "object") continue;
    const id = text(p.id, 32);
    const url = text(p.url, 300);
    const kind = p.kind === "ollama" || p.kind === "openrouter" ? p.kind : "openai";
    if (!/^[A-Za-z0-9_-]+$/.test(id) || !/^https?:\/\/\S+$/.test(url) || providers.some((x) => x.id === id)) continue;
    const start = text(p.start, 500).replace(/[\r\n]+/g, " ");
    providers.push({ id, name: text(p.name, 60) || id, kind, url, ...(start ? { start } : {}) });
  }
  const models: OwnModel[] = [];
  for (const m of Array.isArray(b.models) ? (b.models as Record<string, unknown>[]) : []) {
    if (!m || typeof m !== "object") continue;
    const id = text(m.id, 160);
    if (!id || /\s/.test(id) || models.some((x) => x.id === id)) continue;
    if (!providers.some((p) => id.startsWith(`${p.id}/`))) continue;
    models.push({
      id,
      label: text(m.label, 80) || id,
      local: m.local === true,
      free: m.free === true,
      context: typeof m.context === "number" && Number.isFinite(m.context) ? Math.max(0, Math.round(m.context)) : 0,
    });
  }
  return { providers, models, local: b.local !== false, omniAssist: b.omniAssist !== false };
}

/** A machine's settings call: null is This PC. */
function call<T>(machine: string | null, cmd: string, args: Values = {}): Promise<T> {
  return machine ? callMaster<T>(machine, cmd, args) : invoke<T>(cmd, args);
}

class SettingsStore {
  value = $state<Settings>(parseSettings(null));
  /** The machine the shown settings are of: null is This PC. */
  machine = $state<string | null>(null);
  /** A machine's settings could not be read or kept (said on the Settings page). */
  error = $state("");
  /** The machine's settings were read: until then changes are only kept in the window, then sent. */
  #read = false;
  #unsent: Values = {};
  #loads = 0;
  /** Changes go to the machine one after another, in the order they were made. */
  #queue: Promise<unknown> = Promise.resolve();

  /** Starts with the window's copy of This PC's settings (at once, before the database answers). */
  load(): void {
    this.value = parseSettings(localStorage.getItem(KEY));
    this.#sync();
  }

  /**
   * Reads the shown machine's settings from its database. This PC's first read moves the window's old copy in
   * (Qanuvi kept them only in the window before).
   */
  async loadMachine(): Promise<void> {
    const machine = shownMachine();
    const load = ++this.#loads;
    if (machine !== this.machine) {
      this.#read = false;
      this.#unsent = {};
    }
    try {
      let kept = await call<Values>(machine, "settings_get");
      if (load !== this.#loads) return;
      if (!machine && Object.keys(kept).length === 0) {
        const moved = machinePart(readLocal());
        if (Object.keys(moved).length) await call(null, "settings_set", { values: moved });
        if (load !== this.#loads) return;
        kept = moved;
      }
      const unsent = this.machine === machine ? this.#unsent : {};
      this.machine = machine;
      this.error = "";
      this.value = parseSettings(JSON.stringify({ ...kept, ...unsent, ...this.#windowPart() }));
      this.#read = true;
      this.#unsent = {};
      if (Object.keys(unsent).length) this.#send(machine, unsent);
      this.#keepLocal();
      this.#sync();
    } catch (e) {
      if (load !== this.#loads) return;
      this.error = e instanceof Error ? e.message : String(e);
      // Another machine's settings are never shown as this one's: its page shows the defaults and says why.
      if (machine !== this.machine) {
        this.machine = machine;
        this.value = parseSettings(JSON.stringify(this.#windowPart()));
        this.#sync();
      }
    }
  }

  #windowPart(): Values {
    const v = this.value as unknown as Values;
    return Object.fromEntries(WINDOW_KEYS.map((k) => [k, v[k]]));
  }

  /** The window's copy: its own keys always, This PC's settings when This PC is shown. */
  #keepLocal(): void {
    const base = this.machine ? readLocal() : machinePart(this.value as unknown as Values);
    localStorage.setItem(KEY, JSON.stringify({ ...base, ...this.#windowPart() }));
  }

  #send(machine: string | null, values: Values): void {
    this.#queue = this.#queue
      .then(() => call(machine, "settings_set", { values }))
      .catch((e) => {
        if (this.machine === machine) this.error = e instanceof Error ? e.message : String(e);
      });
  }

  /** The agent list shows the chosen models of Qanuvi's own agent. */
  #sync(): void {
    own.models = this.value.brain.models.map((m) => ({ id: m.id, label: m.label }));
  }

  #save(next: Settings): void {
    const before = this.value as unknown as Values;
    this.value = parseSettings(JSON.stringify(next));
    const changed = changedKeys(before, this.value as unknown as Values);
    this.#keepLocal();
    this.#sync();
    if (!Object.keys(changed).length) return;
    if (this.#read) this.#send(this.machine, changed);
    else this.#unsent = { ...this.#unsent, ...changed };
  }

  /** Waits until the changes made so far reached the machine (tests, and before the window closes). */
  flushed(): Promise<unknown> {
    return this.#queue;
  }

  set<K extends keyof Settings>(key: K, v: Settings[K]): void {
    this.#save({ ...this.value, [key]: v });
  }

  /** Sets or (with "") removes a project's rule for one kind of work. */
  setRule(projectId: string, type: string, model: string): void {
    const current = { ...(this.value.rules[projectId] ?? {}) };
    if (model) current[type] = model;
    else delete current[type];
    this.#save({ ...this.value, rules: { ...this.value.rules, [projectId]: current } });
  }

  agent(providerId: string): AgentSettings {
    return this.value.agents[providerId] ?? cleanAgent(providerId, null);
  }

  setAgent(providerId: string, patch: Partial<AgentSettings>): void {
    this.#save({
      ...this.value,
      agents: { ...this.value.agents, [providerId]: { ...this.agent(providerId), ...patch } },
    });
  }
}

export const settings = new SettingsStore();
