/**
 * Step-by-step setup guides for the model providers of Qanuvi's own agent (Milestone 24), and the rules Qanuvi uses
 * to recommend free models. The guides ship with Qanuvi (checked on the date they carry) and can be brought up to date
 * by a paid agent, with the user's permission; an update is checked here before it is kept.
 */

export type StepAction = "open" | "copy" | "key" | "check" | "find" | "recommend";

export interface GuideStep {
  title: string;
  text: string;
  action?: StepAction;
  /** For "open": the page; for "copy": the command to copy. */
  url?: string;
  command?: string;
}

export interface Guide {
  /** The provider id in Qanuvi (ollama, openrouter, omniroute). */
  provider: string;
  title: string;
  /** When the steps were last checked (YYYY-MM-DD). */
  checkedOn: string;
  summary: string;
  steps: GuideStep[];
  tips: string[];
}

export const BUILT_IN_GUIDES: Guide[] = [
  {
    provider: "openrouter",
    title: "Free models through OpenRouter",
    checkedOn: "2026-11-10",
    summary:
      "OpenRouter offers many models through one key; the ones ending in :free cost nothing. Without buying credit you get 50 free requests a day (20 a minute); after buying $10 of credit once, 1,000 a day.",
    steps: [
      {
        title: "Create an OpenRouter account",
        text: "Sign up on openrouter.ai (Google, GitHub or e-mail). No payment is needed for free models.",
        action: "open",
        url: "https://openrouter.ai",
      },
      {
        title: "Decide on the free models' data setting",
        text: "Some free models are only served by providers that may log prompts or use them for training. If you accept that, allow it in Settings → Privacy; otherwise fewer free models will answer. Either way, do not send passwords or secrets to free models.",
        action: "open",
        url: "https://openrouter.ai/settings/privacy",
      },
      {
        title: "Create a key",
        text: 'On the Keys page press Create, name it "Qanuvi" and give it a small credit limit (free models use none). Copy the key now and keep it secret; if it is ever exposed, delete it there and make a new one.',
        action: "open",
        url: "https://openrouter.ai/keys",
      },
      {
        title: "Give the key to Qanuvi",
        text: "Paste it into the key field and press Save key. It goes to your system's credential store, never into Qanuvi's settings.",
        action: "key",
      },
      {
        title: "Check the key",
        text: "Qanuvi asks OpenRouter whether the key works and how many free requests are left today.",
        action: "check",
      },
      {
        title: "Find the free models",
        text: "Qanuvi lists the free models that can use tools (an agent needs them).",
        action: "find",
      },
      {
        title: "Use the recommended ones",
        text: "Qanuvi ranks them (coding models, bigger models, longer context; tiny ones left out) and adds the best three, with OpenRouter's own free router last as a fallback. You can reorder or remove them.",
        action: "recommend",
      },
      {
        title: "Start working",
        text: 'Choose "Qanuvi agent" for new sessions above, model Auto. When a free model is rate limited, Qanuvi core rests it and the next one takes over.',
      },
    ],
    tips: [
      "Only models ending in :free are free; Qanuvi recommends only those, so nothing is charged by accident.",
      "Free limits are per account: more keys do not add requests.",
      "Many free models at once: put several in the list, so a rate-limited one is skipped instead of stopping your work.",
    ],
  },
  {
    provider: "omniroute",
    title: "Free providers through OmniRoute",
    checkedOn: "2026-11-10",
    summary:
      "OmniRoute (MIT licence) runs on your computer and puts many providers, including free tiers, behind one OpenAI-compatible address, with its own fallback between them. You connect each free provider in its dashboard.",
    steps: [
      {
        title: "Install Node.js",
        text: "OmniRoute needs Node.js 22.22.2 or newer (or 24).",
        action: "open",
        url: "https://nodejs.org",
      },
      {
        title: "Install OmniRoute",
        text: "Run this in a terminal (Qanuvi's terminal works). With npm 11 and later, allow its install scripts if npm asks.",
        action: "copy",
        command: "npm install -g omniroute",
      },
      {
        title: "Start it",
        text: "Find models (below) starts it for you when it is installed but not running. To start it yourself, run this; its dashboard is at http://localhost:20128.",
        action: "copy",
        command: "omniroute serve --daemon",
      },
      {
        title: "Connect free providers",
        text: "In the dashboard, open the free tiers page and connect the providers you want. Each needs its own sign-up; some need a key, Pollinations needs none, and you can add your OpenRouter key for its :free models.",
        action: "open",
        url: "http://localhost:20128/dashboard/free-tiers",
      },
      {
        title: "Copy OmniRoute's key",
        text: "In the dashboard, Endpoints shows the key that apps like Qanuvi use.",
        action: "open",
        url: "http://localhost:20128",
      },
      {
        title: "Give the key to Qanuvi",
        text: "Paste it into the key field and press Save key (kept in your system's credential store).",
        action: "key",
      },
      {
        title: "Find the models",
        text: "Qanuvi lists what OmniRoute offers: your connected models and its routes such as auto/coding.",
        action: "find",
      },
      {
        title: "Use the recommended ones",
        text: "Qanuvi prefers OmniRoute's auto/coding route (it falls back between your free providers by itself), then coding models.",
        action: "recommend",
      },
    ],
    tips: [
      "If the address differs, use the one OmniRoute prints when it starts.",
      "OmniRoute's auto route needs at least one connected provider; connect one first.",
      "Whether tools work depends on the model behind a route; Qanuvi core moves away from models that keep breaking tool calls.",
    ],
  },
  {
    provider: "ollama",
    title: "Models on this computer with Ollama",
    checkedOn: "2026-11-10",
    summary:
      "Ollama runs models on your own computer: free, private and offline. A graphics card helps but is not required; pick a model size that fits your memory.",
    steps: [
      {
        title: "Install Ollama",
        text: "Download it for your system and start it.",
        action: "open",
        url: "https://ollama.com/download",
      },
      {
        title: "Get a coding model",
        text: "About 8 GB of memory (or a 4 GB graphics card): qwen2.5-coder:3b. 16 GB: qwen2.5-coder:7b. 32 GB or a large graphics card: qwen2.5-coder:14b. Run the pull in a terminal; the download is 2 to 9 GB.",
        action: "copy",
        command: "ollama pull qwen2.5-coder:7b",
      },
      {
        title: "Find the models",
        text: "Qanuvi lists the models Ollama has and which of them can use tools.",
        action: "find",
      },
      {
        title: "Use one",
        text: "Press Use on the model. With Auto, questions go to it first; it is free and nothing leaves your computer.",
        action: "recommend",
      },
    ],
    tips: [
      "Small models are fast for questions and simple edits; for bigger changes add a free model from OpenRouter or OmniRoute after it.",
    ],
  },
];

const DATE = /^\d{4}-\d{2}-\d{2}$/;
const ACTIONS: StepAction[] = ["open", "copy", "key", "check", "find", "recommend"];

/**
 * Checks guides (from an update): known providers only, plain text of limited length, pages on http(s) only (http only
 * for this computer), commands on one line. Returns the guides, or the first problem.
 */
export function parseGuides(raw: unknown): Guide[] | string {
  const list = Array.isArray(raw) ? raw : raw && typeof raw === "object" ? (raw as { guides?: unknown }).guides : null;
  if (!Array.isArray(list) || !list.length) return "No guides in the answer.";
  const text = (v: unknown, max: number) => (typeof v === "string" ? v.trim().slice(0, max) : "");
  const out: Guide[] = [];
  for (const g of list as Record<string, unknown>[]) {
    if (!g || typeof g !== "object") return "A guide is not an object.";
    const provider = text(g.provider, 32);
    if (!BUILT_IN_GUIDES.some((b) => b.provider === provider)) return `Unknown provider ${provider || "(none)"}.`;
    if (out.some((x) => x.provider === provider)) return `${provider} appears twice.`;
    const checkedOn = text(g.checkedOn, 10);
    if (!DATE.test(checkedOn)) return `${provider}: checkedOn must be a date like 2026-11-10.`;
    const steps: GuideStep[] = [];
    for (const s of Array.isArray(g.steps) ? (g.steps as Record<string, unknown>[]) : []) {
      if (!s || typeof s !== "object") continue;
      const step: GuideStep = { title: text(s.title, 120), text: text(s.text, 800) };
      if (!step.title || !step.text) return `${provider}: a step needs a title and a text.`;
      const action = text(s.action, 20);
      if (action) {
        if (!ACTIONS.includes(action as StepAction)) return `${provider}: unknown action ${action}.`;
        step.action = action as StepAction;
      }
      const url = text(s.url, 300);
      if (url) {
        const local = /^http:\/\/(localhost|127\.0\.0\.1)(:\d+)?(\/\S*)?$/.test(url);
        if (!/^https:\/\/[^\s/]+(\/\S*)?$/.test(url) && !local) return `${provider}: ${url} is not a web address.`;
        step.url = url;
      }
      const command = text(s.command, 200);
      if (command) {
        if (/[\r\n]/.test(command)) return `${provider}: a command must be one line.`;
        step.command = command;
      }
      if (step.action === "open" && !step.url) return `${provider}: "${step.title}" opens a page but has no url.`;
      if (step.action === "copy" && !step.command) return `${provider}: "${step.title}" has no command.`;
      steps.push(step);
      if (steps.length >= 15) break;
    }
    if (steps.length < 2) return `${provider}: too few steps.`;
    out.push({
      provider,
      title: text(g.title, 100) || provider,
      checkedOn,
      summary: text(g.summary, 800),
      steps,
      tips: (Array.isArray(g.tips) ? g.tips : [])
        .map((t) => text(t, 300))
        .filter(Boolean)
        .slice(0, 8),
    });
  }
  return out;
}

/** The text a paid agent is asked to bring the guides up to date with. */
export function updatePrompt(current: Guide[], today: string): string {
  return [
    "Bring these setup guides for Qanuvi (a code editor) up to date. Qanuvi's own agent uses models from OpenRouter, OmniRoute and Ollama.",
    "Look up the current official pages on the web (openrouter.ai and its docs, OmniRoute's GitHub README and docs, ollama.com) and check each step: sign-up, privacy settings for free models, creating and pasting keys, free-tier limits, install commands, addresses and ports, and how to pick good free models that can use tools for coding.",
    "Change only what is wrong or out of date; keep the same JSON shape, the same providers and these step actions: open (needs url), copy (needs command), key, check, find, recommend. Use plain, short sentences for people new to this. Do not change any files.",
    `Set checkedOn to ${today} for every guide you checked.`,
    "Answer with only the JSON (an object with a guides array), nothing before or after it.",
    "",
    JSON.stringify({ guides: current }, null, 1),
  ].join("\n");
}

/** Finds the JSON object in an agent's answer (it may be wrapped in a code fence or text). */
export function jsonIn(answer: string): unknown {
  const fenced = /```(?:json)?\s*([\s\S]*?)```/.exec(answer);
  const body = fenced ? fenced[1] : answer;
  const start = body.indexOf("{");
  const end = body.lastIndexOf("}");
  if (start < 0 || end <= start) return null;
  try {
    return JSON.parse(body.slice(start, end + 1));
  } catch {
    return null;
  }
}

/** What changed between two versions of a guide, in a few words. */
export function changes(before: Guide | undefined, after: Guide): string {
  if (!before) return "new";
  const parts: string[] = [];
  const titles = (g: Guide) => g.steps.map((s) => s.title);
  const added = titles(after).filter((t) => !titles(before).includes(t)).length;
  const removed = titles(before).filter((t) => !titles(after).includes(t)).length;
  const edited = after.steps.filter((s) => {
    const old = before.steps.find((o) => o.title === s.title);
    return old && JSON.stringify(old) !== JSON.stringify(s);
  }).length;
  if (added) parts.push(`${added} new step${added > 1 ? "s" : ""}`);
  if (removed) parts.push(`${removed} removed`);
  if (edited) parts.push(`${edited} changed`);
  if (before.summary !== after.summary) parts.push("summary changed");
  if (JSON.stringify(before.tips) !== JSON.stringify(after.tips)) parts.push("tips changed");
  return parts.length ? parts.join(", ") : "no changes";
}

/** A model a provider offers, as Qanuvi lists it. */
export interface FoundModel {
  id: string;
  endpoint: string;
  name: string;
  local: boolean;
  free: boolean;
  tools: boolean;
  context: number;
}

export interface Pick {
  model: FoundModel;
  score: number;
  reasons: string[];
}

/** Billions of parameters in a model's name (the largest number before "b"), 0 when not given. */
export function sizeOf(name: string): number {
  let best = 0;
  for (const m of name.toLowerCase().matchAll(/(?:^|[^a-z0-9.])(\d+(?:\.\d+)?)b(?![a-z])/g))
    best = Math.max(best, Number(m[1]));
  return best;
}

/**
 * How well a free model suits Qanuvi's agent: it must use tools and cost nothing; coding models, bigger models and a
 * long context count for it; tiny models and previews count against it. What the brain learned (turns finished) counts
 * too. OpenRouter's free router and OmniRoute's routes are fallbacks or first choices by their nature.
 */
export function score(m: FoundModel, learned?: { turns: number; finished: number }): Pick | null {
  if (!m.tools || !m.free) return null;
  const n = m.name.toLowerCase();
  const reasons: string[] = [];
  let s = 0;
  if (n === "openrouter/free")
    return { model: m, score: 1, reasons: ["OpenRouter picks a free model each time: a good last fallback"] };
  if (n === "auto/coding")
    return { model: m, score: 20, reasons: ["OmniRoute's coding route, with its own fallback between providers"] };
  if (n === "auto") return { model: m, score: 20, reasons: ["OmniRoute's general route, with its own fallback"] };
  if (/(coder|code|codestral|devstral)/.test(n)) {
    s += 5;
    reasons.push("made for code");
  }
  // Model families whose names carry no size (checked 2026-11-10; routers such as OmniRoute offer them for free
  // through sign-ins like Antigravity).
  if (/claude-(opus|sonnet)|gemini-[\d.]+-pro|gemini-pro|gpt-5/.test(n)) {
    // Above any model ranked by size alone.
    s += 20;
    reasons.push("a frontier model");
  } else if (/gemini-[\d.]+-flash(?!-lite)|kimi-k\d|glm-[45]|deepseek-(v\d|r\d|chat)|qwen3[\w.-]*coder/.test(n)) {
    s += 6;
    reasons.push("a strong model family");
  }
  if (/flash-lite|haiku|-low\b|-lite\b/.test(n)) s -= 2;
  if (/thinking/.test(n)) {
    s -= 1;
    reasons.push("thinks before answering (slower)");
  }
  const size = sizeOf(n);
  if (size) {
    s += Math.min(8, Math.log2(size) * 1.4);
    if (size < 4) {
      s -= 8;
      reasons.push(`small (${size}B)`);
    } else if (size >= 25) reasons.push(`large (${size}B)`);
  }
  if (m.context >= 128_000) {
    s += 1;
    reasons.push(`${Math.round(m.context / 1000)}k context`);
  } else if (m.context && m.context < 32_000) s -= 2;
  if (/preview|experimental|\bexp\b/.test(n)) s -= 1;
  if (m.local) {
    s += 1;
    reasons.push("on this computer");
  }
  if (learned && learned.turns >= 3) {
    const rate = learned.finished / learned.turns;
    s += (rate - 0.5) * 6;
    reasons.push(`finished ${learned.finished} of ${learned.turns} turns here`);
  }
  return { model: m, score: Math.round(s * 10) / 10, reasons };
}

/** The best free models to use, best first (at most `n`), with OpenRouter's free router kept for last. */
export function recommend(
  models: FoundModel[],
  learned: Record<string, { turns: number; finished: number }> = {},
  n = 3,
): Pick[] {
  const picks = models.map((m) => score(m, learned[m.id])).filter((p): p is Pick => !!p && p.score > -3);
  const router = picks.find((p) => p.model.name === "openrouter/free");
  const rest = picks.filter((p) => p !== router).sort((a, b) => b.score - a.score);
  const top = rest.slice(0, router ? n - 1 : n);
  return router ? [...top, router] : top;
}

/** A model Ollama offers, with what it is good for and its download size (ollama.com, checked 2026-11-10). */
export interface LocalModel {
  name: string;
  role: "coder" | "chat";
  sizeGb: number;
  thinking: boolean;
  note: string;
}

export const LOCAL_MODELS_CHECKED_ON = "2026-11-10";

/** Best first within each role; all can use tools (the agent needs them). */
export const LOCAL_MODELS: LocalModel[] = [
  {
    name: "qwen3-coder:30b",
    role: "coder",
    sizeGb: 19,
    thinking: false,
    note: "coding model; quick for its size (3B active)",
  },
  {
    name: "devstral-small-2:24b",
    role: "coder",
    sizeGb: 15,
    thinking: false,
    note: "made for coding agents; long context",
  },
  { name: "qwen3.5:9b", role: "coder", sizeGb: 7.6, thinking: true, note: "recent all-round model, good at code" },
  { name: "qwen2.5-coder:7b", role: "coder", sizeGb: 4.7, thinking: false, note: "proven coding model" },
  { name: "qwen3.5:4b", role: "coder", sizeGb: 4.0, thinking: true, note: "recent small model" },
  { name: "qwen2.5-coder:3b", role: "coder", sizeGb: 1.9, thinking: false, note: "small coding model; simple edits" },
  {
    name: "qwen2.5-coder:1.5b",
    role: "coder",
    sizeGb: 1.0,
    thinking: false,
    note: "very small; questions and tiny edits",
  },
  { name: "qwen3.8:27b", role: "chat", sizeGb: 18, thinking: true, note: "newest large Qwen; deep thinking" },
  { name: "gemma4:26b", role: "chat", sizeGb: 19, thinking: true, note: "large Gemma; quick for its size" },
  { name: "qwen3.5:9b", role: "chat", sizeGb: 7.6, thinking: true, note: "conversation and thinking" },
  { name: "qwen3:8b", role: "chat", sizeGb: 5.2, thinking: true, note: "conversation and thinking" },
  { name: "qwen3:4b", role: "chat", sizeGb: 2.5, thinking: true, note: "small, thinks before answering" },
  { name: "qwen3:1.7b", role: "chat", sizeGb: 1.4, thinking: true, note: "very small" },
];

export interface MachineInfo {
  ramGb: number;
  gpu: { name: string; vramGb: number; kind: string } | null;
  os: string;
  arch: string;
}

export interface LocalSuggestion {
  model: LocalModel;
  /** Fits the graphics card (fast) or only the main memory (works, slower). */
  fast: boolean;
}

/**
 * The best coder and chat model this computer can run: what fits the graphics card's memory (with room for the
 * context), otherwise what fits about half the main memory, which runs on the processor and is slower.
 */
export function suggestLocal(m: MachineInfo, catalogue: LocalModel[] = LOCAL_MODELS) {
  const gpuRoom = m.gpu ? m.gpu.vramGb * 0.85 : 0;
  const ramRoom = m.ramGb * 0.45;
  const pick = (role: LocalModel["role"]): LocalSuggestion | null => {
    const list = catalogue.filter((x) => x.role === role);
    const fast = list.find((x) => x.sizeGb <= gpuRoom);
    if (fast) return { model: fast, fast: true };
    const slow = list.find((x) => x.sizeGb <= ramRoom);
    return slow ? { model: slow, fast: false } : null;
  };
  return { coder: pick("coder"), chat: pick("chat") };
}

/**
 * Free providers worth connecting in OmniRoute for coding (checked 2026-11-10; to be updated from qanuvi.com). Each has
 * a free tier with its own limits; "key" ones need a key from their site, "sign-in" ones a sign-in in the browser.
 */
export interface FreeProvider {
  /** OmniRoute's provider id. */
  id: string;
  name: string;
  how: "key" | "sign-in";
  /** Where the key is made (or the sign-in happens). */
  url: string;
  note: string;
  /** Qanuvi's own provider whose kept key can be reused (no copying). */
  reuse?: string;
}

export const FREE_PROVIDERS_CHECKED_ON = "2026-11-10";

export const FREE_PROVIDERS: FreeProvider[] = [
  {
    id: "antigravity",
    name: "Antigravity (Google sign-in)",
    how: "sign-in",
    url: "https://antigravity.google",
    note: "Free Claude Sonnet/Opus and Gemini through a Google account; the strongest free models. Sign-ins expire and need renewing.",
  },
  {
    id: "openrouter",
    name: "OpenRouter",
    how: "key",
    url: "https://openrouter.ai/keys",
    note: "Many :free models; 50 requests a day (1,000 after buying $10 once).",
    reuse: "openrouter",
  },
  {
    id: "gemini",
    name: "Gemini (Google AI Studio)",
    how: "key",
    url: "https://aistudio.google.com/apikey",
    note: "Free Gemini Flash with a daily limit; a key from AI Studio.",
  },
  {
    id: "groq",
    name: "Groq",
    how: "key",
    url: "https://console.groq.com/keys",
    note: "Very fast open models (Llama, Qwen, Kimi) with free limits per minute and day.",
  },
  {
    id: "cerebras",
    name: "Cerebras",
    how: "key",
    url: "https://cloud.cerebras.ai",
    note: "Fast open models (Qwen coder, Llama) on a free tier.",
  },
  {
    id: "nvidia",
    name: "NVIDIA NIM",
    how: "key",
    url: "https://build.nvidia.com",
    note: "Free credits for many open models, including large coding ones.",
  },
  {
    id: "mistral",
    name: "Mistral",
    how: "key",
    url: "https://console.mistral.ai/api-keys",
    note: "Free experiment plan including Codestral and Devstral (coding models).",
  },
];

/**
 * What local models can honestly do on this computer (the setup wizard says it plainly): "coding" when a capable
 * coder (about 15 GB and up, like qwen3-coder:30b) runs fast here; "questions" when only small models fit — they answer
 * questions and make small edits but are not good enough for real coding; "none" when nothing useful fits.
 */
export function localVerdict(m: MachineInfo): { level: "none" | "questions" | "coding"; text: string } {
  const { coder } = suggestLocal(m);
  if (!coder)
    return {
      level: "none",
      text: "This computer has too little memory for a useful local model. Use the free online models: they are stronger anyway.",
    };
  if (coder.model.sizeGb >= 15 && coder.fast)
    return {
      level: "coding",
      text: `This computer can run ${coder.model.name}, a capable coder, fast on its graphics card. For the biggest jobs the online models (Claude through OmniRoute) are still stronger.`,
    };
  return {
    level: "questions",
    text: `Models that fit here (like ${coder.model.name}) answer questions and make small edits for free and offline, but they are not good enough for real coding work. For that, Qanuvi uses the free online models (Claude through OmniRoute, OpenRouter).`,
  };
}
