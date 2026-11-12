/**
 * Turns the agent's streamed JSON messages (and Qanuvi's own notes of the user's messages and answers) into the items
 * the agent pane shows: the user's messages, the agent's text, quiet step lines for tools, approval cards and a
 * result line with time, tokens and cost.
 */

import { withoutOverruleNote } from "$lib/core/decider";
import { withoutGuidance } from "$lib/plan/guidance";
import type { Edit } from "./diff";
import { CHAT_HANDOVER_MARK, parseResume, tokensK } from "./handover";

/** An image in a message: stored in the data folder (`file`), or still in hand (`src`, a data address). */
export interface ShownImage {
  name: string;
  file?: string;
  src?: string;
}

/** Every item may carry the time it arrived (seconds since 1970), for the timeline. */
export type Item = (
  | {
      kind: "user";
      id: string;
      text: string;
      attachments?: string[];
      images?: ShownImage[];
      during?: boolean;
      /**
       * Sent while the agent worked and not taken in yet (24r): "sent" until the step in hand ends, "next" once it did;
       * the agent's next answer has read it.
       */
      waiting?: "sent" | "next";
    }
  /** live: text still being written (replaced by the whole message when it arrives). */
  | { kind: "text"; id: string; text: string; live?: boolean }
  | {
      kind: "tool";
      id: string;
      name: string;
      summary: string;
      /** File the tool works on, when there is one (to open it from the step). */
      file: string | null;
      /** stopped: ended by "Stop this command, read my message now" (24r), not a failure. */
      state: "running" | "done" | "failed" | "stopped";
      /** The result's first line, for the folded card. */
      result: string;
      /** IN: the command or what the tool was given, as text. */
      input: string;
      /** OUT: the whole result (cut at a length a card can show). */
      output: string;
      /** A file change (Edit, MultiEdit, Write): what it replaced and put there, shown as a diff. */
      edits?: Edit[];
      /** The change wrote the whole file (its old text is not known: no Revert from the chat). */
      whole?: boolean;
      /** A screenshot the tool answered with, as a data address. */
      image?: string;
    }
  | {
      kind: "approval";
      id: string;
      tool: string;
      summary: string;
      detail: string;
      state: "pending" | "allowed" | "refused";
      /** Allowed by an earlier "for this session" / "for 10 minutes". */
      auto?: boolean;
    }
  /** The model's thinking, shown folded. */
  | { kind: "thinking"; id: string; text: string; live?: boolean }
  /** The agent's to-do list (TodoWrite): one card, updated in place each time the agent writes the list again. */
  | { kind: "todos"; id: string; todos: Todo[] }
  | {
      kind: "result";
      id: string;
      ok: boolean;
      seconds: number;
      costUsd: number;
      text: string;
      /** The turn ended because the user stopped the command in hand (24r). */
      stopped?: boolean;
    }
  /** note: a chat handover's note (24s), shown folded under the line. */
  | { kind: "notice"; id: string; text: string; tone: "info" | "error"; model?: string; note?: string }
) & {
  at?: number;
  /** From an earlier session of the chat (24s): read only. */
  older?: boolean;
};

export interface Todo {
  text: string;
  status: "pending" | "in_progress" | "completed";
}

type Json = Record<string, unknown>;

/** Longest IN and OUT a step card keeps (the whole conversation stays in the session log). */
const IN_MAX = 4000;
const OUT_MAX = 8000;

const cutAt = (t: string, max: number) =>
  t.length > max ? `${t.slice(0, max)}\n… (${t.length - max} more characters)` : t;

/** IN for a step card: the command for shells, the edit for file changes, the address or element for the test browser. */
export function toolInput(name: string, input: Json): string {
  const short = shortToolName(name);
  if (/^browser_/.test(short)) {
    const parts = ["url", "selector", "number", "text", "value", "expression"]
      .filter((k) => input[k] !== undefined && input[k] !== "")
      .map((k) => `${k}: ${typeof input[k] === "string" ? input[k] : JSON.stringify(input[k])}`);
    return cutAt(parts.join("\n"), IN_MAX);
  }
  if (str(input.file_path) && !str(input.old_string) && !str(input.content) && !str(input.new_string)) {
    const range =
      input.offset || input.limit
        ? ` (from line ${Number(input.offset ?? 1)}${input.limit ? `, ${Number(input.limit)} lines` : ""})`
        : "";
    return str(input.file_path) + range;
  }
  return cutAt(toolDetail(short, input), IN_MAX);
}

/** Longest text of one edit kept for the diff. */
const EDIT_MAX = 20000;

/** The changes a file tool makes, for the diff in its card. */
export function editsOf(name: string, input: Json): { edits: Edit[]; whole?: boolean } | null {
  const short = shortToolName(name);
  const one = (e: Json): Edit | null =>
    typeof e.new_string === "string" || typeof e.old_string === "string"
      ? { old: str(e.old_string), new: str(e.new_string), ...(e.replace_all === true ? { all: true } : {}) }
      : null;
  if (short === "Edit") {
    const e = one(input);
    return e ? { edits: [e] } : null;
  }
  if (short === "MultiEdit" && Array.isArray(input.edits)) {
    const edits = input.edits
      .map(obj)
      .map(one)
      .filter((e): e is Edit => !!e);
    return edits.length ? { edits } : null;
  }
  if (short === "Write" && typeof input.content === "string") {
    const content = input.content.length > EDIT_MAX ? input.content.slice(0, EDIT_MAX) : input.content;
    return { edits: [{ old: "", new: content }], whole: true };
  }
  return null;
}

/** The to-do list a TodoWrite call carries. */
function todosOf(input: Json): Todo[] {
  return (Array.isArray(input.todos) ? input.todos : [])
    .map(obj)
    .map((t) => ({
      text: str(t.status) === "in_progress" && str(t.activeForm) ? str(t.activeForm) : str(t.content),
      status: (["pending", "in_progress", "completed"].includes(str(t.status))
        ? str(t.status)
        : "pending") as Todo["status"],
    }))
    .filter((t) => t.text);
}

const str = (v: unknown) => (typeof v === "string" ? v : "");
const obj = (v: unknown): Json => (v && typeof v === "object" ? (v as Json) : {});

function firstLine(text: string, max = 160): string {
  const line = text.split(/\r?\n/).find((l) => l.trim()) ?? "";
  return line.length > max ? `${line.slice(0, max)}…` : line;
}

/** A one-line description of what a tool call does. */
export function toolSummary(name: string, input: Json): string {
  const pick = str(input.description) || str(input.command) || str(input.file_path) || str(input.notebook_path);
  if (pick) return firstLine(pick);
  const other = str(input.pattern) || str(input.url) || str(input.query) || str(input.prompt) || str(input.path);
  if (other) return firstLine(other);
  const json = JSON.stringify(input);
  return json === "{}" ? name : firstLine(json, 120);
}

function toolDetail(name: string, input: Json): string {
  if (str(input.command)) return str(input.command);
  const cut = (t: string, max: number) => (t.length > max ? `${t.slice(0, max)}\n…` : t);
  if (str(input.file_path) && typeof input.old_string === "string") {
    return `${str(input.file_path)}\n\nReplace:\n${cut(str(input.old_string), 600)}\n\nWith:\n${cut(str(input.new_string), 600)}`;
  }
  if (str(input.file_path) && typeof input.content === "string" && name === "Write") {
    return `${str(input.file_path)} (the whole file becomes)\n\n${cut(str(input.content), 1200)}`;
  }
  if (str(input.file_path)) {
    const body = str(input.content) || str(input.new_string);
    return body
      ? `${str(input.file_path)}\n\n${body.slice(0, 1200)}${body.length > 1200 ? "\n…" : ""}`
      : str(input.file_path);
  }
  const json = JSON.stringify(input, null, 2);
  return json.length > 1500 ? `${json.slice(0, 1500)}\n…` : json;
}

function resultText(content: unknown): string {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) return content.map((c) => str(obj(c).text)).join("\n");
  return "";
}

/** The first picture in a tool's answer, as a data address. */
export function resultImage(content: unknown): string {
  if (!Array.isArray(content)) return "";
  for (const c of content) {
    const b = obj(c);
    if (b.type !== "image") continue;
    const src = obj(b.source);
    const data = str(src.data) || str(b.data);
    const type = str(src.media_type) || str(b.mimeType) || "image/png";
    if (data && /^image\/(png|jpeg|gif|webp)$/.test(type)) return `data:${type};base64,${data}`;
  }
  return "";
}

/**
 * How long a running step has taken, for its card (24r): nothing for the first 10 seconds, then "42 s", "12 min",
 * "1 h 05 min".
 */
export function elapsedLabel(seconds: number): string {
  const s = Math.floor(seconds);
  if (!(s >= 10)) return "";
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} min`;
  return `${Math.floor(m / 60)} h ${String(m % 60).padStart(2, "0")} min`;
}

/** A tool's name without the MCP server prefix: "mcp__qanuvi-browser__browser_open" → "browser_open". */
/** How a step ended by "Stop this command, read my message now" reads. */
const STOPPED = "Stopped by you";
/** Claude Code's result for the step in hand when a turn is interrupted. */
const STOP_TEXT = /^The user doesn't want to proceed with this tool use/;

export function shortToolName(name: string): string {
  const m = /^mcp__[^_]+(?:_[^_]+)*?__(.+)$/.exec(name);
  return m ? m[1] : name;
}

/**
 * Messages sent while the agent worked (24r): Claude Code reads them only once the step in hand ends. A tool's result
 * or the turn's end marks that; the agent's next answer has then read the message, which moves to just before that
 * answer (where it was read) and leaves the waiting area.
 */
function takeIn(items: Item[], type: string, msg: Json): Item[] {
  if (!items.some((x) => x.kind === "user" && x.waiting)) return items;
  const toolResult =
    type === "user" &&
    Array.isArray(obj(msg.message).content) &&
    (obj(msg.message).content as unknown[]).some((b) => obj(b).type === "tool_result");
  if (toolResult || type === "result" || type === "qanuvi_interrupted")
    return items.map((x) => (x.kind === "user" && x.waiting === "sent" ? { ...x, waiting: "next" as const } : x));
  if (type !== "assistant") return items;
  const read = items.filter((x) => x.kind === "user" && x.waiting === "next");
  if (!read.length) return items;
  return [
    ...items.filter((x) => !read.includes(x)),
    ...read.map((x) => {
      const { waiting: _, ...rest } = x as Extract<Item, { kind: "user" }>;
      return rest;
    }),
  ];
}

/** Adds one message to the list of items. Returns a new array. */
export function reduce(items: Item[], line: string, seq: number, at = 0): Item[] {
  const out = reduceLine(items, line, seq);
  if (!at || out === items) return out;
  // New items get the time; items updated in place keep the time they started.
  const known = new Set(items.map((x) => x.id));
  return out.map((x) => (known.has(x.id) || x.at ? x : { ...x, at }));
}

function reduceLine(items: Item[], line: string, seq: number): Item[] {
  let msg: Json;
  try {
    msg = obj(JSON.parse(line));
  } catch {
    return items;
  }
  const type = str(msg.type);

  // Text as it is written: added to the live text at the end.
  if (type === "stream_event") {
    const delta = obj(obj(msg.event).delta);
    const last = items[items.length - 1];
    // Thinking as it is written: shown folded, like the finished thinking that takes its place.
    if (delta.type === "thinking_delta" && str(delta.thinking)) {
      if (last?.kind === "thinking" && last.live)
        return [...items.slice(0, -1), { ...last, text: last.text + str(delta.thinking) }];
      return [...items, { kind: "thinking", id: `livethink${seq}`, text: str(delta.thinking), live: true }];
    }
    if (delta.type !== "text_delta" || !str(delta.text)) return items;
    if (last?.kind === "text" && last.live)
      return [...items.slice(0, -1), { ...last, text: last.text + str(delta.text) }];
    return [...items, { kind: "text", id: `live${seq}`, text: str(delta.text), live: true }];
  }
  // Anything else ends the live text: the whole message (or a handover, or the end) takes its place.
  let out = items.filter((x) => !((x.kind === "text" || x.kind === "thinking") && x.live));
  out = takeIn(out, type, msg);
  if (type === "qanuvi_user" && str(msg.text).startsWith(CHAT_HANDOVER_MARK)) {
    const size = /about (\d+)k tokens/.exec(str(msg.text))?.[1];
    out.push({
      kind: "notice",
      id: `ho${seq}`,
      text: `Handing over to a new session${size ? ` (context ${size}k)` : ""}: the agent finishes the edit in hand and writes a note.`,
      tone: "info",
    });
  } else if (type === "qanuvi_user" && parseResume(str(msg.text))) {
    const r = parseResume(str(msg.text))!;
    out.push({
      kind: "notice",
      id: `nw${seq}`,
      text: `New session · context carried over${r.was ? ` (was ${tokensK(r.was)})` : ""}`,
      tone: "info",
      note: r.note,
    });
  } else if (type === "qanuvi_user") {
    const attachments = Array.isArray(msg.attachments) ? msg.attachments.map(str).filter(Boolean) : [];
    const images: ShownImage[] = Array.isArray(msg.images)
      ? msg.images
          .map(obj)
          .map((i) => ({
            name: str(i.name),
            ...(str(i.file) ? { file: str(i.file) } : {}),
            ...(str(i.src).startsWith("data:image/") ? { src: str(i.src) } : {}),
          }))
          .filter((i) => i.file || i.src)
      : [];
    out.push({
      kind: "user",
      id: `u${seq}`,
      text: withoutGuidance(withoutOverruleNote(str(msg.text))),
      ...(attachments.length ? { attachments } : {}),
      ...(images.length ? { images } : {}),
      ...(msg.during === true ? { during: true, waiting: "sent" as const } : {}),
    });
  } else if (type === "qanuvi_interrupted") {
    // "Stop this command, read my message now": the steps still running were ended.
    for (let i = 0; i < out.length; i++) {
      const x = out[i];
      if (x.kind === "tool" && x.state === "running") out[i] = { ...x, state: "stopped", result: STOPPED, output: "" };
    }
    out.push({
      kind: "notice",
      id: `int${seq}`,
      text: "Stopped the command in hand; the agent reads your message next.",
      tone: "info",
    });
  } else if (type === "qanuvi_permission") {
    const i = out.findIndex((x) => x.kind === "approval" && x.id === str(msg.request_id));
    if (i >= 0)
      out[i] = {
        ...(out[i] as Extract<Item, { kind: "approval" }>),
        state: msg.allow ? "allowed" : "refused",
        auto: msg.auto === true,
      };
  } else if (type === "assistant") {
    const content = obj(msg.message).content;
    for (const [n, block] of (Array.isArray(content) ? content : []).entries()) {
      const b = obj(block);
      if (b.type === "text" && str(b.text).trim()) {
        out.push({ kind: "text", id: `t${seq}.${n}`, text: str(b.text) });
      } else if (b.type === "thinking" && str(b.thinking).trim()) {
        out.push({ kind: "thinking", id: `th${seq}.${n}`, text: str(b.thinking) });
      } else if (b.type === "tool_use" && shortToolName(str(b.name)) === "TodoWrite") {
        // The list, not a step: the first card is kept and updated, so it reads as one live checklist.
        const todos = todosOf(obj(b.input));
        const i = out.findIndex((x) => x.kind === "todos");
        if (i >= 0) out[i] = { ...(out[i] as Extract<Item, { kind: "todos" }>), todos };
        else out.push({ kind: "todos", id: `todo${seq}.${n}`, todos });
      } else if (b.type === "tool_use") {
        const input = obj(b.input);
        out.push({
          kind: "tool",
          id: str(b.id) || `tool${seq}.${n}`,
          name: shortToolName(str(b.name)),
          summary: toolSummary(str(b.name), input),
          file: str(input.file_path) || str(input.notebook_path) || null,
          state: "running",
          result: "",
          input: toolInput(str(b.name), input),
          output: "",
          ...(editsOf(str(b.name), input) ?? {}),
        });
      }
    }
  } else if (type === "user") {
    const content = obj(msg.message).content;
    for (const block of Array.isArray(content) ? content : []) {
      const b = obj(block);
      if (b.type !== "tool_result") continue;
      const i = out.findIndex((x) => x.kind === "tool" && x.id === str(b.tool_use_id));
      if (i >= 0) {
        const tool = out[i] as Extract<Item, { kind: "tool" }>;
        const image = resultImage(b.content);
        // Claude Code answers an interrupt with its "rejected" text for the step in hand: show it as stopped.
        if (b.is_error && STOP_TEXT.test(resultText(b.content))) {
          out[i] = { ...tool, state: "stopped", result: STOPPED, output: "" };
          continue;
        }
        out[i] = {
          ...tool,
          state: b.is_error ? "failed" : "done",
          result: firstLine(resultText(b.content), 200),
          output: cutAt(resultText(b.content).replace(/\s+$/, ""), OUT_MAX),
          ...(image ? { image } : {}),
        };
      }
    }
  } else if (type === "control_request" && obj(msg.request).subtype === "can_use_tool") {
    const req = obj(msg.request);
    const input = obj(req.input);
    out.push({
      kind: "approval",
      id: str(msg.request_id),
      tool: str(req.display_name) || str(req.tool_name),
      summary: str(req.description) || toolSummary(str(req.tool_name), input),
      detail: toolDetail(str(req.tool_name), input),
      state: "pending",
    });
  } else if (type === "result") {
    // A turn the user interrupted ended as asked: no error line.
    // (the stop notice comes after the turn's last result; the stopped step's own result may follow it)
    const since = out.findLastIndex((x) => x.kind === "result") + 1;
    const interrupted = out.slice(since).some((x) => x.kind === "notice" && x.id.startsWith("int"));
    if (interrupted) msg = { ...msg, is_error: false };
    // Approvals still pending when a turn ends were never answered.
    for (let i = 0; i < out.length; i++) {
      const x = out[i];
      if (x.kind === "tool" && x.state === "running") out[i] = { ...x, state: "done" };
    }
    out.push({
      kind: "result",
      id: `r${seq}`,
      ok: !msg.is_error,
      seconds: Number(msg.duration_ms ?? 0) / 1000,
      costUsd: Number(msg.total_cost_usd ?? 0),
      text: msg.is_error ? firstLine(str(msg.result) || str(msg.subtype), 300) : "",
      ...(interrupted ? { stopped: true } : {}),
    });
  } else if (type === "system" && str(msg.subtype) === "qanuvi_route") {
    // Qanuvi's brain gave the turn to a model (first pick or a handover), with the reason.
    const model = str(msg.model) || undefined;
    out.push({ kind: "notice", id: `n${seq}`, text: str(msg.text), tone: "info", model });
  } else if (type === "system" && str(msg.subtype) === "init") {
    // Qanuvi's own agent starts as "Qanuvi brain (auto)": (older sessions: "Qanuvi brain (auto)"); the actual model is named per turn.
    const model = /^Qanuvi (brain|core) /.test(str(msg.model)) ? undefined : str(msg.model) || undefined;
    out.push({ kind: "notice", id: `n${seq}`, text: `Started with ${str(msg.model)}`, tone: "info", model });
  }
  return out;
}

export function reduceAll(lines: string[], times: number[] = []): Item[] {
  return lines.reduce<Item[]>((items, line, i) => reduce(items, line, i, times[i] ?? 0), []);
}

type Tool = Extract<Item, { kind: "tool" }>;

/** A row of the timeline: an item, several look-around steps in a row as one, or a long run of steps folded. */
export type Row =
  | Item
  | { kind: "group"; id: string; label: string; steps: Tool[]; state: Tool["state"]; at?: number }
  | { kind: "fold"; id: string; count: number; rows: Row[] };

const READING = /^(Read|read_lines|outline|NotebookRead)$/;
const SEARCHING = /^(Grep|Glob|LS|search|find_symbol|project_map|memory_search|WebSearch)$/;
const PAGE = /^(browser_read|browser_links|browser_fields|WebFetch)$/;

/** Steps that only look around (read, search, read a page): several in a row read as one line. */
export function isLook(step: Tool): boolean {
  return READING.test(step.name) || SEARCHING.test(step.name) || PAGE.test(step.name);
}

/** "Read 5 files · searched 2 times". */
export function lookLabel(steps: Tool[]): string {
  const files = new Set(steps.filter((s) => READING.test(s.name)).map((s) => s.file ?? s.summary)).size;
  const searches = steps.filter((s) => SEARCHING.test(s.name)).length;
  const pages = steps.filter((s) => PAGE.test(s.name)).length;
  const times = (n: number) => (n === 1 ? "once" : `${n} times`);
  const parts = [
    files ? `Read ${files} file${files === 1 ? "" : "s"}` : "",
    searches ? `searched ${times(searches)}` : "",
    pages ? `read the page ${times(pages)}` : "",
  ].filter(Boolean);
  const text = parts.join(" · ");
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/** Look-around steps in a row become one group from this many. */
export const GROUP_FROM = 3;
/** A run of steps longer than this (between messages) folds, keeping the last few in view. */
export const FOLD_OVER = 10;
export const FOLD_KEEP = 4;

/**
 * The items as timeline rows: look-around steps in a row grouped, and long runs of steps folded so the newest stay in
 * view. A failed or running look step stays in its group (the group shows the state).
 */
export function timelineRows(items: Item[]): Row[] {
  const grouped: Row[] = [];
  let run: Tool[] = [];
  const flush = () => {
    if (run.length >= GROUP_FROM) {
      const state = run.some((s) => s.state === "running")
        ? "running"
        : run.some((s) => s.state === "failed")
          ? "failed"
          : "done";
      grouped.push({ kind: "group", id: `g${run[0].id}`, label: lookLabel(run), steps: run, state, at: run[0].at });
    } else grouped.push(...run);
    run = [];
  };
  for (const item of items) {
    if (item.kind === "tool" && isLook(item)) run.push(item);
    else {
      flush();
      grouped.push(item);
    }
  }
  flush();
  // Long runs of steps (and the quiet lines between them) fold, the last few stay.
  const out: Row[] = [];
  let steps: Row[] = [];
  const isStep = (r: Row) => r.kind === "tool" || r.kind === "group" || r.kind === "thinking";
  const end = () => {
    if (steps.filter(isStep).length > FOLD_OVER) {
      const hidden = steps.slice(0, steps.length - FOLD_KEEP);
      out.push({ kind: "fold", id: `f${hidden[0].id}`, count: hidden.filter(isStep).length, rows: hidden });
      out.push(...steps.slice(steps.length - FOLD_KEEP));
    } else out.push(...steps);
    steps = [];
  };
  for (const r of grouped) {
    if (isStep(r) || r.kind === "notice" || (r.kind === "approval" && r.state !== "pending")) steps.push(r);
    else {
      end();
      out.push(r);
    }
  }
  end();
  return out;
}

/** The time of a message: the clock time today, with the date on other days. */
export function clockTime(at: number, now = Date.now()): string {
  const d = new Date(at * 1000);
  const time = d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  return new Date(now).toDateString() === d.toDateString()
    ? time
    : `${d.toLocaleDateString([], { day: "numeric", month: "short" })} ${time}`;
}

export function formatTokens(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

export function formatCost(usd: number): string {
  if (usd === 0) return "$0";
  if (usd < 0.01) return `$${usd.toFixed(4)}`;
  return `$${usd.toFixed(2)}`;
}

/** The conversation as Markdown, for "Export conversation". */
export function itemsToMarkdown(title: string, items: Item[]): string {
  const out = [`# ${title}`, ""];
  for (const item of items) {
    if (item.kind === "user") out.push(`**You:** ${item.text}`, "");
    else if (item.kind === "text") out.push(item.text, "");
    else if (item.kind === "tool")
      out.push(
        `- ${item.name}: ${item.summary}${item.state === "failed" ? " (failed)" : item.state === "stopped" ? " (stopped)" : ""}`,
      );
    else if (item.kind === "approval")
      out.push(
        `- Approval for ${item.tool}: ${item.summary} (${item.state === "allowed" ? "approved" : item.state === "refused" ? "refused" : "not answered"})`,
      );
    else if (item.kind === "todos")
      out.push(...item.todos.map((t) => `- [${t.status === "completed" ? "x" : " "}] ${t.text}`), "");
    else if (item.kind === "result")
      out.push(
        "",
        `_${item.stopped ? "Stopped by you" : item.ok ? "Finished" : "Stopped with an error"} in ${item.seconds.toFixed(1)} s_`,
        "",
      );
  }
  return (
    out
      .join("\n")
      .replace(/\n{3,}/g, "\n\n")
      .trim() + "\n"
  );
}

/** The model working on the conversation now: the last one the agent named (Qanuvi's brain names each one it picks). */
export function currentModel(items: Item[]): string {
  for (let i = items.length - 1; i >= 0; i--) {
    const x = items[i];
    if (x.kind === "notice" && x.model) return x.model;
  }
  return "";
}

/** Live text that is a tool call being written as JSON (small models do that): not worth showing as it arrives. */
export function looksLikeCall(text: string): boolean {
  return /^\s*(\{|\[|<tool_call|```(json)?\s*\{)/.test(text);
}
