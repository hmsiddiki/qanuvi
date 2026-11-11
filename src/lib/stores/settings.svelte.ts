/** User preferences kept in local storage. */
import { DEFAULT_TEAM, parseTeam } from "$lib/core/decider";
import { PROVIDERS, providerById } from "$lib/agents/providers";
import { own } from "$lib/agents/ownModels.svelte";

const KEY = "qanuvi.settings.v1";

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
  /** How Qanuvi looks (15h): the editor's font size (px) and the size of the whole interface (%). */
  appearance: { editorFontSize: number; uiScale: number };
  /** Start with Qanuvi: commands started hidden when Qanuvi starts (a Headroom proxy, for example). */
  startup: StartupItem[];
  /** Updates (1b): off, a newer version waits for the user's click in the status bar before it downloads. */
  updates: { autoDownload: boolean };
}

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
  name: string;
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
  appearance: { editorFontSize: 13, uiScale: 100 },
  updates: { autoDownload: false },
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
    s.appearance = { editorFontSize: n(ap.editorFontSize, 13, 10, 24), uiScale: n(ap.uiScale, 100, 80, 150) };
  }
  {
    const up = raw.updates && typeof raw.updates === "object" ? (raw.updates as Record<string, unknown>) : {};
    s.updates = { autoDownload: up.autoDownload === true };
  }
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
        name: str(m.name).slice(0, 60) || str(m.target),
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

class SettingsStore {
  value = $state<Settings>(parseSettings(null));

  load(): void {
    this.value = parseSettings(localStorage.getItem(KEY));
    this.#sync();
  }

  /** The agent list shows the chosen models of Qanuvi's own agent. */
  #sync(): void {
    own.models = this.value.brain.models.map((m) => ({ id: m.id, label: m.label }));
  }

  #save(next: Settings): void {
    this.value = parseSettings(JSON.stringify(next));
    localStorage.setItem(KEY, JSON.stringify(this.value));
    this.#sync();
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
