/** The user's memory in the window (Milestone 17): the slice agents get, and the entries for the Memory view. */
import { api } from "$lib/api";
import { settings } from "./settings.svelte";

/** The memory to send with a request, or null (memory off, nothing applies, or it could not be read). */
export async function memoryContext(projectId: string | null, request: string): Promise<string | null> {
  if (!settings.value.memoryContext) return null;
  try {
    const slice = await api.memory.slice(projectId, request, settings.value.memoryBudget, true);
    return slice.text || null;
  } catch {
    return null;
  }
}

class MemoryStore {
  /** Goes up when entries change (an agent suggested one, the user edited one), so views refresh. */
  changed = $state(0);
  /** How many agent suggestions wait for the user. */
  waiting = $state(0);

  async refreshWaiting(): Promise<void> {
    try {
      const all = await api.memory.list(null, true);
      this.waiting = all.filter((e) => e.status === "suggested").length;
    } catch {
      this.waiting = 0;
    }
  }

  touch(): void {
    this.changed++;
    this.refreshWaiting();
  }
}

export const memory = new MemoryStore();
