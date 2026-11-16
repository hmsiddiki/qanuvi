import { api, type Shell } from "$lib/api";
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

  async loadShells(): Promise<void> {
    try {
      this.shells = await api.term.shells();
    } catch {
      this.shells = [];
    }
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

  /** Its shell was stopped (the terminal panel was closed): a new screen starts a new one. */
  stopped(key: string): void {
    const t = this.tabs.find((x) => x.key === key);
    if (t) t.termId = undefined;
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
    const label = onServer ? "Server terminal" : (this.shells.find((s) => s.id === shell)?.label ?? "Terminal");
    const key = `term-${++counter}`;
    this.tabs = [
      ...this.tabs,
      {
        key,
        projectId,
        shell,
        title: dir ? `${label} · ${dir.split("/").pop()}` : label,
        ended: false,
        dir,
        machine: switcher.shown,
      },
    ];
    this.activeByProject = { ...this.activeByProject, [projectId]: key };
  }

  select(projectId: string, key: string): void {
    this.activeByProject = { ...this.activeByProject, [projectId]: key };
  }

  close(key: string): void {
    const tab = this.tabs.find((t) => t.key === key);
    this.tabs = this.tabs.filter((t) => t.key !== key);
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
