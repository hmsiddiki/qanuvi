import type { Shell } from "$lib/api";
import { settings } from "./settings.svelte";
import { switcher } from "./switcher.svelte";
import { callOn } from "$lib/remote";

export interface TerminalTab {
  key: string;
  projectId: string;
  shell: string;
  title: string;
  ended: boolean;
  /** A folder of the project to start in (right-click → Open in terminal); the project's top otherwise. */
  dir?: string;
  /** The machine it runs on: null is This PC (24z3 Part B2). */
  machine: string | null;
  /** The running terminal's id on its machine, once it started. */
  termId?: string;
  /** Its machine is hidden: no screen in the window, the shell goes on there and its output is kept. */
  parked?: boolean;
  /** m3: opened before the machine's shells were known, so titled "Terminal"; named once they are (loadShells). */
  unnamed?: boolean;
}

/** A tab's title: the shell's name, with the folder it started in. */
function tabTitle(label: string, dir?: string): string {
  return dir ? `${label} · ${dir.split("/").pop()}` : label;
}

let counter = 0;

/**
 * Terminal tabs. Each project has its own; they keep running while another project is shown. Only the shown
 * machine's have a screen (24z3 Part B2): a hidden machine's are parked and given their recent output on return.
 */
export class TerminalsStore {
  tabs = $state<TerminalTab[]>([]);
  activeByProject = $state<Record<string, string>>({});
  shells = $state<Shell[]>([]);

  /** The shown machine's shells (24z3 step 3, f2): read again on each switch; a late answer for another is dropped. */
  async loadShells(): Promise<void> {
    const machine = switcher.shown;
    let found: Shell[];
    try {
      // An answer with no list (null, as a master that is not ready gives) is no shells, never a crash.
      const got = await callOn<Shell[] | null>(machine, "term_shells", {});
      found = Array.isArray(got) ? got : [];
    } catch {
      found = [];
    }
    if (switcher.shown !== machine) return;
    this.shells = found;
    // m3: tabs opened (or brought back) before the list came get their shell's name now; an empty shell is the
    // machine's default, the first in the list.
    if (!found.length || !this.tabs.some((t) => t.unnamed && t.machine === machine)) return;
    this.tabs = this.tabs.map((t) => {
      if (!t.unnamed || t.machine !== machine) return t;
      const shell = found.find((s) => s.id === t.shell) ?? (t.shell === "" ? found[0] : undefined);
      return shell ? { ...t, title: tabTitle(shell.label, t.dir), unnamed: false } : t;
    });
  }

  /** The tabs that have a screen now: the shown machine's. */
  get mounted(): TerminalTab[] {
    return this.tabs.filter((t) => t.machine === switcher.shown);
  }

  /** The terminal started on its machine. */
  started(key: string, termId: string): void {
    const t = this.tabs.find((x) => x.key === key);
    if (t) t.termId = termId;
  }

  /**
   * Its screen went while the tab stays (the terminal panel or work pane was closed): the shell goes on, its output
   * kept, and the next screen attaches to it again.
   */
  parkOne(key: string): void {
    const t = this.tabs.find((x) => x.key === key);
    if (!t) return;
    t.parked = true;
    if (t.termId && !t.ended) void callOn(t.machine, "term_detach", { id: t.termId }).catch(() => {});
  }

  /** Its screen is back and shows the terminal again. */
  attached(key: string): void {
    const t = this.tabs.find((x) => x.key === key);
    if (t) t.parked = false;
  }

  /**
   * The switcher hides a machine: its terminals leave the window but go on there, their output kept for the return.
   * Marked before the screens go, so they are not stopped; the machine is told only while it can be reached.
   */
  async park(machine: string | null, reachable: boolean): Promise<void> {
    const mine = this.tabs.filter((t) => t.machine === machine);
    for (const t of mine) t.parked = true;
    if (!reachable) return;
    // Named machine, not the shown one: the switch goes on meanwhile.
    const running = mine.filter((t) => t.termId && !t.ended);
    await Promise.all(running.map((t) => callOn(machine, "term_detach", { id: t.termId }).catch(() => {})));
  }

  /**
   * Stops this window's terminals on This PC (before its interface reloads; the list is kept, so they start again
   * afterwards). M5a: they run in This PC's master, which would otherwise keep them until the link ends.
   */
  async stopThisPc(): Promise<void> {
    const running = this.tabs.filter((t) => t.machine === null && t.termId && !t.ended);
    await Promise.all(running.map((t) => callOn(null, "term_kill", { id: t.termId }).catch(() => {})));
  }

  /** A machine was removed from the switcher: its tabs go (its terminals ended with its connection). */
  forgetMachine(machine: string): void {
    this.tabs = this.tabs.filter((t) => t.machine !== machine);
  }

  forProject(projectId: string | null): TerminalTab[] {
    return this.tabs.filter((t) => t.projectId === projectId);
  }

  active(projectId: string | null): string | null {
    if (!projectId) return null;
    const key = this.activeByProject[projectId];
    return this.tabs.some((t) => t.key === key) ? key : (this.forProject(projectId)[0]?.key ?? null);
  }

  /** Projects on a server: their terminals run the server's own default shell (this PC's list does not apply). */
  remote: (projectId: string) => boolean = () => false;

  open(projectId: string, shellId?: string, dir?: string): void {
    const onServer = this.remote(projectId);
    const shell = onServer ? "" : shellId || settings.value.defaultShell || this.shells[0]?.id || "";
    const known = this.shells.find((s) => s.id === shell);
    const label = onServer ? "Server terminal" : (known?.label ?? "Terminal");
    const key = `term-${++counter}`;
    this.tabs = [
      ...this.tabs,
      {
        key,
        projectId,
        shell,
        title: tabTitle(label, dir),
        ended: false,
        dir,
        machine: switcher.shown,
        ...(!onServer && !known ? { unnamed: true } : {}),
      },
    ];
    this.activeByProject = { ...this.activeByProject, [projectId]: key };
  }

  select(projectId: string, key: string): void {
    this.activeByProject = { ...this.activeByProject, [projectId]: key };
  }

  /**
   * Tabs closed by the user. A plain set, not state: a screen's teardown reads the state as it was before the change
   * that removed it (Svelte 5), so the closed tab still looked listed there and its shell was parked, not stopped.
   */
  #closed = new Set<string>();

  /** The user closed this tab (its shell is stopped by close itself). */
  wasClosed(key: string): boolean {
    return this.#closed.has(key);
  }

  close(key: string): void {
    const tab = this.tabs.find((t) => t.key === key);
    this.#closed.add(key);
    this.tabs = this.tabs.filter((t) => t.key !== key);
    // Its shell stops here, shown or parked; one still starting is stopped by its screen when its id comes.
    if (tab?.termId && !tab.ended) void callOn(tab.machine, "term_kill", { id: tab.termId }).catch(() => {});
    if (tab && this.activeByProject[tab.projectId] === key) {
      const next = this.forProject(tab.projectId).at(-1);
      const map = { ...this.activeByProject };
      if (next) map[tab.projectId] = next.key;
      else delete map[tab.projectId];
      this.activeByProject = map;
    }
  }

  forgetProject(projectId: string): void {
    this.tabs = this.tabs.filter((t) => t.projectId !== projectId);
  }
}

export const terminals = new TerminalsStore();
