/**
 * i1 (migration M5c): a window that goes keeps its unsaved text as drafts on the shown machine, as the switcher does
 * when it hides a machine (24z3 Part B2). They come back in their files when the project opens again, marked "Draft,
 * not saved to disk", and the Inbox says which files were kept.
 */
import type { Draft } from "$lib/api";
import { machineName, shownMachine } from "$lib/remote";
import { drafts } from "./drafts";
import { editor } from "./editor.svelte";
import { inbox } from "./inbox.svelte";
import { projects } from "./projects.svelte";
import { switcher } from "./switcher.svelte";

/** How many files the note names; the rest are counted. */
const NAMED = 8;

/** The Inbox note for drafts kept when a window went (null when none were kept). */
export function keptNote(
  kept: Draft[],
  machine: string | null,
  projectName: (id: string) => string,
): { title: string; text: string } | null {
  if (!kept.length) return null;
  const where = machine ? machineName(machine) : "This PC";
  const label = (d: Draft) => {
    if (!d.projectId) {
      try {
        return (JSON.parse(d.meta || "{}") as { label?: string }).label || d.path;
      } catch {
        return d.path;
      }
    }
    return `${d.path} (${projectName(d.projectId)})`;
  };
  const named = kept.slice(0, NAMED).map((d) => `- ${label(d)}`);
  if (kept.length > NAMED) named.push(`- and ${kept.length - NAMED} more`);
  const files = kept.length === 1 ? "1 file" : `${kept.length} files`;
  return {
    title: `Unsaved changes to ${files} kept as drafts on ${where}`,
    text:
      `The window closed or started again with unsaved changes. They are kept on ${where} as drafts, not saved to disk, and come ` +
      `back in their files when the project opens again:\n${named.join("\n")}`,
  };
}

/** The close step: keeps the drafts on the shown machine and puts the note in its Inbox. */
export async function keepUnsavedOnClose(): Promise<void> {
  const unsaved = editor.unsaved();
  if (!unsaved.length) return;
  const machine = shownMachine();
  // A machine held as lost (24z3 Part E) cannot take them, and the window that would hold them is going.
  if (switcher.held?.id === machine) return;
  const left = await drafts.keep(machine, unsaved, true);
  const kept = unsaved.filter((d) => !left.includes(d));
  const note = keptNote(kept, machine, (id) => projects.list.find((p) => p.id === id)?.name ?? "its project");
  if (note) await inbox.addNow({ kind: "drafts", key: `drafts-kept:${machine ?? "this-pc"}`, ...note }, machine);
}
