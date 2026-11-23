import { isOpenReview, type InboxItem } from "$lib/stores/inbox.svelte";

/** A step as the review list names it: its number, else its title. */
export const stepKey = (s: { number?: string | null; title: string }) =>
  s.number ? `#${s.number}` : `t:${s.title.trim()}`;

/**
 * Ticked steps that still have things open (half-orange in the step list): what the plan's open review list names
 * for them, the work a step left for later that no step does, the checks that need a person's eyes and the agent's
 * "To look at:" lines. "All fine" (or a typed answer) closes the list and the steps show green again.
 */
export function openThings(items: InboxItem[], projectId: string, file: string): Map<string, string[]> {
  const out = new Map<string, string[]>();
  for (const i of items) {
    if (!isOpenReview(i) || i.source?.kind !== "review") continue;
    if (i.source.projectId !== projectId || i.source.file !== file) continue;
    for (const e of i.source.entries) {
      const k = stepKey(e);
      const list = out.get(k) ?? [];
      if (!list.includes(e.what)) out.set(k, [...list, e.what]);
    }
  }
  return out;
}
