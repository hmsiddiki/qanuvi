/**
 * Export a conversation (24n): the chat as one Markdown file a person can read, keep or share. The owner's messages
 * and the agent's answers in full; each step as one line, with a shell command and a file change shown as code; the
 * output only of steps that failed (cut short). Thinking, approvals and Qanuvi's own notices are left out.
 */
import type { Item } from "./events";

/** Lines of a failed step's output an export keeps. */
const FAILED_LINES = 20;

const fence = (lang: string, text: string) => {
  // A fence longer than any run of backticks inside, so the text cannot end it early.
  const longest = Math.max(2, ...[...text.matchAll(/`+/g)].map((m) => m[0].length));
  const f = "`".repeat(longest + 1);
  return `${f}${lang}\n${text.replace(/\n+$/, "")}\n${f}`;
};

function stepLines(item: Extract<Item, { kind: "tool" }>): string[] {
  const state = item.state === "failed" ? " (failed)" : item.state === "stopped" ? " (stopped)" : "";
  const out = [`- **${item.name}** ${item.summary}${state}`.trimEnd()];
  if (item.edits?.length) {
    const diff = item.edits
      .map((e) => [
        ...e.old
          .split("\n")
          .filter((l, i, all) => l || i < all.length - 1)
          .map((l) => `- ${l}`),
        ...e.new
          .split("\n")
          .filter((l, i, all) => l || i < all.length - 1)
          .map((l) => `+ ${l}`),
      ])
      .flat()
      .join("\n");
    if (diff) out.push("", fence("diff", diff), "");
  } else if (/^(Bash|PowerShell|shell|run_command)$/i.test(item.name) && item.input.trim()) {
    out.push("", fence("sh", item.input.trim()), "");
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
 * go in the line under it.
 */
export function exportMarkdown(items: Item[], title: string, when: string, agent: string): string {
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
        out.push(...stepLines(item));
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
