/**
 * Unsaved editor text of a machine the switcher hides (24z3 Part B2). It is kept on the machine the files belong to
 * and given back when the window shows that machine again; nothing is asked. A machine that cannot be reached any more
 * (its connection ended before it was hidden) has its drafts held in this window until it is shown again. Calls name
 * their machine (never "the shown one"), so the switch does not wait for them; they run one after another, so a
 * machine shown again at once reads what was just kept.
 */
import type { Draft } from "$lib/api";
import { callOn } from "$lib/remote";

/** How long keeping or reading drafts may take before going on without them. */
const WAIT_MS = 4000;

function within<T>(p: Promise<T>, ms = WAIT_MS): Promise<T> {
  return Promise.race([p, new Promise<T>((_, no) => setTimeout(() => no(new Error("No answer in time.")), ms))]);
}

/** The window's key for a machine: "" is This PC. */
const slot = (machine: string | null) => machine ?? "";

export class DraftsStore {
  /** Drafts of machines that could not be reached, by machine. */
  #held = new Map<string, Draft[]>();
  /** The work before: keeping and taking run in order. */
  #queue: Promise<unknown> = Promise.resolve();

  #next<T>(work: () => Promise<T>): Promise<T> {
    const run = this.#queue.then(work, work);
    this.#queue = run.catch(() => {});
    return run;
  }

  /** Keeps drafts on their machine; held in the window when it cannot be reached (or does not answer). */
  keep(machine: string | null, drafts: Draft[], reachable: boolean): Promise<void> {
    if (!drafts.length) return this.#queue.then(() => {});
    return this.#next(async () => {
      const failed: Draft[] = [];
      if (reachable)
        await Promise.all(
          drafts.map((d) =>
            within(callOn(machine, "drafts_put", { projectId: d.projectId, draft: d })).catch(
              () => void failed.push(d),
            ),
          ),
        );
      else failed.push(...drafts);
      if (failed.length) this.#hold(machine, failed);
    });
  }

  #hold(machine: string | null, drafts: Draft[]): void {
    const same = (a: Draft, b: Draft) => a.projectId === b.projectId && a.path === b.path;
    const kept = (this.#held.get(slot(machine)) ?? []).filter((h) => !drafts.some((d) => same(h, d)));
    this.#held.set(slot(machine), [...kept, ...drafts]);
  }

  /**
   * The drafts of one project of a machine (empty project: agent configuration files), taken: the machine's copy is
   * dropped once read, the window's held copy goes too. A held draft wins over the machine's (it is the newer one).
   */
  take(machine: string | null, projectId: string): Promise<Draft[]> {
    return this.#next(async () => {
      const fromMachine = await within(callOn<Draft[]>(machine, "drafts_list", { projectId })).catch(() => null);
      const held = this.#held.get(slot(machine)) ?? [];
      const mine = held.filter((d) => d.projectId === projectId);
      if (mine.length) {
        const rest = held.filter((d) => d.projectId !== projectId);
        if (rest.length) this.#held.set(slot(machine), rest);
        else this.#held.delete(slot(machine));
      }
      const out = [...mine];
      for (const d of fromMachine ?? []) if (!out.some((x) => x.path === d.path)) out.push(d);
      // Dropped before they are used, so a draft kept again right after (its file did not open) is not dropped too.
      await Promise.all(
        (fromMachine ?? []).map((d) =>
          within(callOn(machine, "drafts_drop", { projectId: d.projectId, path: d.path })).catch(() => {}),
        ),
      );
      return out;
    });
  }

  /** Drafts held in the window for a machine. */
  held(machine: string | null): Draft[] {
    return this.#held.get(slot(machine)) ?? [];
  }
}

export const drafts = new DraftsStore();
