/**
 * Export a conversation (24n): the chat as one Markdown file a person can read, keep or share. The owner's messages
 * and the agent's answers in full; each step as one line, with a shell command and a file change shown as code; the
 * output only of steps that failed (cut short). Thinking, approvals and Qanuvi's own notices are left out.
 */
import type { Item } from "./events";
import { editDiff } from "./diff";

/** Lines of a failed step's output an export keeps. */
const FAILED_LINES = 20;

const fence = (lang: string, text: string) => {
  // A fence longer than any run of backticks inside, so the text cannot end it early.
  const longest = Math.max(2, ...[...text.matchAll(/`+/g)].map((m) => m[0].length));
  const f = "`".repeat(longest + 1);
  return `${f}${lang}\n${text.replace(/\n+$/, "")}\n${f}`;
};

function stepLines(item: Extract<Item, { kind: "tool" }>, short: (t: string) => string): string[] {
  const state = item.state === "failed" ? " (failed)" : item.state === "stopped" ? " (stopped)" : "";
  const out = [`- **${item.name}** ${short(item.summary)}${state}`.trimEnd()];
  if (item.edits?.length) {
    const mark = { add: "+ ", del: "- ", ctx: "  ", gap: "…" } as const;
    const diff = item.edits
      .flatMap((e) => editDiff(e.old, e.new).map((l) => (l.type === "gap" ? mark.gap : mark[l.type] + l.text)))
      .join("\n");
    if (diff) out.push("", fence("diff", diff), "");
  } else if (/^(Bash|PowerShell|shell|run_command)$/i.test(item.name) && item.input.trim()) {
    out.push("", fence("sh", short(item.input.trim())), "");
  }
  if (item.state === "failed" && item.output.trim()) {
    const lines = item.output.trim().split("\n");
    const kept = lines.slice(0, FAILED_LINES).join("\n");
    out.push(
      "",
      fence("", lines.length > FAILED_LINES ? `${kept}\n… (${lines.length - FAILED_LINES} more lines)` : kept),
      "",
    );
  }
  return out;
}

/**
 * The conversation as Markdown. `title` heads the file; `when` and `agent` (for example "Claude Code · Sonnet 5.5")
 * go in the line under it; `root` (the project's folder) is left out of the paths in steps.
 */
export function exportMarkdown(items: Item[], title: string, when: string, agent: string, root = ""): string {
  // Paths inside the project read from its folder ("src/notes.js"), whichever slashes the agent used.
  const base = root.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
  const short = (t: string) => {
    if (!base) return t;
    // Compared with forward slashes and case aside; the text keeps its own characters elsewhere.
    const flat = t.replace(/\\/g, "/").toLowerCase();
    let out = "";
    let from = 0;
    for (let at = flat.indexOf(`${base}/`); at >= 0; at = flat.indexOf(`${base}/`, from)) {
      out += t.slice(from, at);
      from = at + base.length + 1;
    }
    return out + t.slice(from);
  };
  const out: string[] = [`# ${title.trim() || "Conversation"}`, "", `Exported from Qanuvi, ${when}. Agent: ${agent}.`];
  let last: "user" | "agent" | null = null;
  const head = (who: "user" | "agent") => {
    if (last === who) return;
    out.push("", who === "user" ? "## You" : "## Agent", "");
    last = who;
  };
  for (const item of items) {
    switch (item.kind) {
      case "user":
        if (item.withdrawn) break;
        last = null;
        head("user");
        out.push(item.text.trim());
        if (item.attachments?.length) out.push("", `Attached: ${item.attachments.join(", ")}`);
        break;
      case "text":
        if (item.live || !item.text.trim()) break;
        head("agent");
        out.push(item.text.trim(), "");
        break;
      case "tool":
        head("agent");
        out.push(...stepLines(item, short));
        break;
      case "todos":
        head("agent");
        out.push(...item.todos.map((t) => `- [${t.status === "completed" ? "x" : " "}] ${t.text}`), "");
        break;
      case "result":
        head("agent");
        out.push(
          "",
          `_${item.stopped ? "Stopped" : item.ok ? "Finished" : "Ended with an error"} · ${item.seconds.toFixed(1)} s${item.costUsd ? ` · $${item.costUsd.toFixed(4)}` : ""}_`,
        );
        break;
      default:
        break;
    }
  }
  return (
    out
      .join("\n")
      .replace(/\n{3,}/g, "\n\n")
      .trim() + "\n"
  );
}

/** A file name for the export: the title's words, safe on every system, and the date. */
export function exportFileName(title: string, date: string): string {
  const words =
    title
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .slice(0, 50)
      .replace(/-+$/, "") || "conversation";
  return `${words}-${date}.md`;
}
