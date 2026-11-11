/**
 * Turns the agent's streamed JSON messages (and Qanuvi's own notes of the user's messages and answers) into the items
 * the agent pane shows: the user's messages, the agent's text, quiet step lines for tools, approval cards and a
 * result line with time, tokens and cost.
 */

import { withoutOverruleNote } from "$lib/core/decider";

export type Item =
  | { kind: "user"; id: string; text: string }
  /** live: text still being written (replaced by the whole message when it arrives). */
  | { kind: "text"; id: string; text: string; live?: boolean }
  | {
      kind: "tool";
      id: string;
      name: string;
      summary: string;
      /** File the tool works on, when there is one (to open it from the step). */
      file: string | null;
      state: "running" | "done" | "failed";
      result: string;
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
  | { kind: "result"; id: string; ok: boolean; seconds: number; costUsd: number; text: string }
  | { kind: "notice"; id: string; text: string; tone: "info" | "error"; model?: string };

type Json = Record<string, unknown>;

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

/** A tool's name without the MCP server prefix: "mcp__qanuvi-browser__browser_open" → "browser_open". */
export function shortToolName(name: string): string {
  const m = /^mcp__[^_]+(?:_[^_]+)*?__(.+)$/.exec(name);
  return m ? m[1] : name;
}

/** Adds one message to the list of items. Returns a new array. */
export function reduce(items: Item[], line: string, seq: number): Item[] {
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
    if (delta.type !== "text_delta" || !str(delta.text)) return items;
    const last = items[items.length - 1];
    if (last?.kind === "text" && last.live)
      return [...items.slice(0, -1), { ...last, text: last.text + str(delta.text) }];
    return [...items, { kind: "text", id: `live${seq}`, text: str(delta.text), live: true }];
  }
  // Anything else ends the live text: the whole message (or a handover, or the end) takes its place.
  const out = items.filter((x) => !(x.kind === "text" && x.live));
  if (type === "qanuvi_user") {
    out.push({ kind: "user", id: `u${seq}`, text: withoutOverruleNote(str(msg.text)) });
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
        out[i] = {
          ...tool,
          state: b.is_error ? "failed" : "done",
          result: firstLine(resultText(b.content), 200),
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

export function reduceAll(lines: string[]): Item[] {
  return lines.reduce<Item[]>((items, line, i) => reduce(items, line, i), []);
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
      out.push(`- ${item.name}: ${item.summary}${item.state === "failed" ? " (failed)" : ""}`);
    else if (item.kind === "approval")
      out.push(
        `- Approval for ${item.tool}: ${item.summary} (${item.state === "allowed" ? "approved" : item.state === "refused" ? "refused" : "not answered"})`,
      );
    else if (item.kind === "result")
      out.push("", `_${item.ok ? "Finished" : "Stopped with an error"} in ${item.seconds.toFixed(1)} s_`, "");
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
