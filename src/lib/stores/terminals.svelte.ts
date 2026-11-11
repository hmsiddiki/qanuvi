import { api, type Shell } from "$lib/api";
import { settings } from "./settings.svelte";

export interface TerminalTab {
  key: string;
  projectId: string;
  shell: string;
  title: string;
  ended: boolean;
}

let counter = 0;

/** Terminal tabs. Each project has its own; they keep running while another project is shown. */
class TerminalsStore {
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

  open(projectId: string, shellId?: string): void {
    const onServer = this.remote(projectId);
    const shell = onServer ? "" : shellId || settings.value.defaultShell || this.shells[0]?.id || "";
    const label = onServer ? "Server terminal" : (this.shells.find((s) => s.id === shell)?.label ?? "Terminal");
    const key = `term-${++counter}`;
    this.tabs = [...this.tabs, { key, projectId, shell, title: label, ended: false }];
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
