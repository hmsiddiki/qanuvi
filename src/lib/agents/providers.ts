/**
 * Agents Qanuvi can run. Each agent describes itself here: its models, permission modes, effort levels, settings and
 * configuration files. Qanuvi's own interface reads these descriptions and names no agent itself, so later agents
 * (from extensions) plug in the same way.
 */
import type { Component } from "svelte";
import Hand from "@lucide/svelte/icons/hand";
import CodeXml from "@lucide/svelte/icons/code-xml";
import NotebookPen from "@lucide/svelte/icons/notebook-pen";
import Zap from "@lucide/svelte/icons/zap";
import { own } from "./ownModels.svelte";

export interface AgentModel {
  id: string;
  label: string;
}

export interface AgentMode {
  id: string;
  label: string;
  description: string;
  icon: Component;
}

export interface AgentProvider {
  id: string;
  name: string;
  /** Short words shown next to the name, for example "by its maker" or "command-line agent". */
  kind: string;
  /** Letter or symbol for the agent's badge. */
  badge: string;
  color: string;
  models: AgentModel[];
  defaultModel: string;
  modes: AgentMode[];
  defaultMode: string;
  /** Effort levels, lowest first; empty when the agent has none. */
  efforts: string[];
  defaultEffort: string;
  /** The agent can be pointed at another address (a proxy). */
  supportsBaseUrl: boolean;
  /** Text for the address setting. */
  baseUrlHint: string;
  /** Where to install it when it is not found. */
  installHint: string;
}

export const CLAUDE_CODE: AgentProvider = {
  id: "claude-code",
  name: "Claude Code",
  kind: "Command-line coding agent",
  badge: "C",
  color: "#d97757",
  models: [
    { id: "claude-fable-5-1", label: "Fable 5.1" },
    { id: "claude-opus-5-5", label: "Opus 5.5" },
    { id: "claude-sonnet-5-5", label: "Sonnet 5.5" },
    { id: "claude-haiku-5-5", label: "Haiku 5.5" },
  ],
  defaultModel: "claude-sonnet-5-5",
  modes: [
    {
      id: "default",
      label: "Manual",
      description: "Asks for your approval before each edit and command.",
      icon: Hand,
    },
    {
      id: "acceptEdits",
      label: "Edit automatically",
      description: "Edits files without asking; commands still ask.",
      icon: CodeXml,
    },
    {
      id: "plan",
      label: "Plan",
      description: "Explores the code and presents a plan; changes nothing.",
      icon: NotebookPen,
    },
    {
      id: "auto",
      label: "Auto",
      description: "Approves actions that pass a safety check and pauses for anything risky.",
      icon: Zap,
    },
  ],
  defaultMode: "default",
  efforts: ["low", "medium", "high", "xhigh", "max"],
  defaultEffort: "medium",
  supportsBaseUrl: true,
  baseUrlHint: "A proxy such as Headroom, for example http://127.0.0.1:8787. Empty: the agent's own setting.",
  installHint: "Install Claude Code and make sure the claude command works in a terminal.",
};

/** The paid agent the user has: any agent that is not Qanuvi's own (Claude Code today, others as they come). */
export function paidAgent(): AgentProvider | undefined {
  return PROVIDERS.find((p) => p.id !== "qanuvi");
}

/** The model id that lets Qanuvi's brain choose. */
export const AUTO_MODEL = "auto";

/**
 * Qanuvi's own agent (Milestone 24): local models through Ollama and free or paid ones through OpenRouter, OmniRoute or
 * any OpenAI-compatible API, with Qanuvi's brain choosing and falling back between them.
 */
export const QANUVI_AGENT: AgentProvider = {
  id: "qanuvi",
  name: "Qanuvi agent",
  kind: "Local and free models, chosen by Qanuvi",
  badge: "Q",
  color: "#0e9fb5",
  get models() {
    return [{ id: AUTO_MODEL, label: "Auto (Qanuvi picks)" }, ...own.models];
  },
  defaultModel: AUTO_MODEL,
  modes: CLAUDE_CODE.modes.map((m) =>
    m.id === "auto"
      ? { ...m, description: "Edits and runs commands; asks first for risky ones (deleting, pushing, downloading)." }
      : m,
  ),
  defaultMode: "default",
  efforts: [],
  defaultEffort: "",
  supportsBaseUrl: false,
  baseUrlHint: "",
  installHint: "Add a model below: one on this computer through Ollama, or a free one through OpenRouter or OmniRoute.",
};

export const PROVIDERS: AgentProvider[] = [CLAUDE_CODE, QANUVI_AGENT];

/** Agents planned for later, shown in the Agents panel so the user knows what is coming. */
export const COMING: { name: string; note: string }[] = [
  { name: "Codex CLI", note: "OpenAI's command-line agent" },
  { name: "Gemini CLI", note: "Google's command-line agent" },
  { name: "Agents from extensions", note: "Added by Qanuvi extensions" },
];

export function providerById(id: string | null | undefined): AgentProvider {
  return PROVIDERS.find((p) => p.id === id) ?? CLAUDE_CODE;
}

export function modelLabel(provider: AgentProvider, id: string): string {
  // A plan step that went to Qanuvi's own agent with free models first (plan/runner.ts FREE_MODEL).
  if (id.startsWith("qanuvi:")) return "Qanuvi core (free)";
  // Free decides, paid works: a free model picks the paid agent's model for each message (24b).
  if (id === "auto" && provider.id !== "qanuvi") return "Auto (a free model picks)";
  // Another agent's model (a plan run that used both): its own label.
  return (
    provider.models.find((m) => m.id === id)?.label ??
    PROVIDERS.flatMap((p) => p.models).find((m) => m.id === id)?.label ??
    id
  );
}
