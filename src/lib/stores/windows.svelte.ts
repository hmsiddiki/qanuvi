/**
 * Qanuvi's windows (Milestone 15c): one window is the normal way of working; a project can also be opened in a window
 * of its own, for two projects side by side. Windows tell each other which project they show, so a project open in
 * one window is shown as such in the others and opening it again brings that window forward instead.
 *
 * A machine can have a window of its own too (24z3 step 4, "Open in a new window" in the machine switcher): each
 * window shows one machine through its own switcher, and windows tell each other which machine they show, so picking
 * a machine another window shows brings that window forward. The machine windows still open when the main window
 * closes (the app ends with it) open again at the next start, each on its machine.
 */
import { emit, emitTo, listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow, WebviewWindow } from "@tauri-apps/api/webviewWindow";

interface ProjectNote {
  label: string;
  projectId: string | null;
}

interface MachineNote {
  label: string;
  /** The machine it shows: "" is This PC; null: the window closed. */
  machine: string | null;
}

/** The machine windows open when the main window closed, opened again at the next start. */
export const MACHINE_WINDOWS_KEY = "qanuvi.machineWindows";

/** This PC in a window address (a master id is never this word). */
const THIS_PC = "pc";

/**
 * The machine a window was opened for (`?machine=` in its address): null is This PC, undefined when the address
 * names none (the main window, a project window).
 */
export function machineFromUrl(search: string): string | null | undefined {
  const m = new URLSearchParams(search).get("machine");
  if (m === THIS_PC) return null;
  return m && /^[\w-]{1,80}$/.test(m) ? m : undefined;
}

/** The machine windows kept for the next start: master ids, "" for This PC (bad entries dropped). */
export function parseMachineWindows(raw: string | null): string[] {
  try {
    const list: unknown = JSON.parse(raw ?? "[]");
    if (!Array.isArray(list)) return [];
    const ok = list.filter((m): m is string => typeof m === "string" && (m === "" || /^[\w-]{1,80}$/.test(m)));
    return [...new Set(ok)].slice(0, 8);
  } catch {
    return [];
  }
}

/** The project a window was opened for (`?project=` in its address). */
export function projectFromUrl(search: string): string | null {
  const id = new URLSearchParams(search).get("project");
  return id && /^[\w-]{1,80}$/.test(id) ? id : null;
}

class WindowsStore {
  label = "main";
  /** A project window (not the main one): it does not change which project the main window opens with. */
  secondary = false;
  /** Projects shown in the other windows: window label -> project id. */
  others = $state<Record<string, string>>({});
  /** Machines shown in the other windows: window label -> machine ("" is This PC). */
  otherMachines = $state<Record<string, string>>({});
  #current: () => string | null = () => null;
  #machine: () => string | null = () => null;

  async init(current: () => string | null, machine: () => string | null = () => null): Promise<void> {
    this.#current = current;
    this.#machine = machine;
    try {
      this.label = getCurrentWebviewWindow().label;
    } catch {
      return; // not running inside the app (tests)
    }
    this.secondary = this.label !== "main";
    await listen<ProjectNote>("qanuvi-window-project", (e) => {
      const { label, projectId } = e.payload;
      if (label === this.label) return;
      const next = { ...this.others };
      if (projectId) next[label] = projectId;
      else delete next[label];
      this.others = next;
    });
    await listen<MachineNote>("qanuvi-window-machine", (e) => {
      const { label, machine } = e.payload;
      if (label === this.label) return;
      const next = { ...this.otherMachines };
      if (machine === null) delete next[label];
      else next[label] = machine;
      this.otherMachines = next;
      // The main window keeps the list for the next start current (a window closed by itself leaves it at once).
      if (!this.secondary) this.keepMachineWindows();
    });
    // A new window asks the others what they show.
    await listen<{ label: string }>("qanuvi-window-hello", (e) => {
      if (e.payload.label === this.label) return;
      this.announce(this.#current());
      this.announceMachine(this.#machine());
    });
    await emit("qanuvi-window-hello", { label: this.label });
    window.addEventListener("beforeunload", () => {
      // The main window closing ends the app: the machine windows still open come back at the next start.
      if (!this.secondary) this.keepMachineWindows();
      emit("qanuvi-window-project", { label: this.label, projectId: null });
      emit("qanuvi-window-machine", { label: this.label, machine: null });
    });
  }

  /**
   * A machine window's title follows the machine it shows (finding w6: one that fell back to This PC, its server
   * removed, still had the server's name). The main window and project windows keep theirs.
   */
  titleFor(name: string): void {
    if (!this.label.startsWith("machine-")) return;
    try {
      void getCurrentWebviewWindow()
        .setTitle(`${name} · Qanuvi`)
        .catch(() => {});
    } catch {
      // not running inside the app (tests)
    }
  }

  announceMachine(machine: string | null): void {
    emit("qanuvi-window-machine", { label: this.label, machine: machine ?? "" }).catch(() => {});
  }

  /** The other window showing a machine (null: This PC), if any. */
  windowOfMachine(machine: string | null): string | null {
    const want = machine ?? "";
    return Object.entries(this.otherMachines).find(([, m]) => m === want)?.[0] ?? null;
  }

  #keptMachineWindows(): string[] {
    return Object.entries(this.otherMachines)
      .filter(([label]) => label.startsWith("machine-"))
      .map(([, m]) => m);
  }

  keepMachineWindows(): void {
    try {
      localStorage.setItem(MACHINE_WINDOWS_KEY, JSON.stringify(this.#keptMachineWindows()));
    } catch {
      // No storage: they do not come back.
    }
  }

  /** The machine windows to open again at start (the main window only), read once. */
  takeMachineWindows(): string[] {
    try {
      const list = parseMachineWindows(localStorage.getItem(MACHINE_WINDOWS_KEY));
      localStorage.removeItem(MACHINE_WINDOWS_KEY);
      return list;
    } catch {
      return [];
    }
  }

  /** Opens a machine in a window of its own (null: This PC), or brings forward the window that already shows it. */
  async openMachine(machine: string | null, name: string): Promise<void> {
    const existing = this.windowOfMachine(machine);
    if (existing) return this.focus(existing);
    const label = `machine-${Date.now().toString(36)}`;
    await this.#open(label, `/?machine=${encodeURIComponent(machine ?? THIS_PC)}`, `${name} · Qanuvi`);
  }

  announce(projectId: string | null): void {
    emit("qanuvi-window-project", { label: this.label, projectId }).catch(() => {});
  }

  /** The other window showing a project, if any. */
  windowOf(projectId: string): string | null {
    return Object.entries(this.others).find(([, p]) => p === projectId)?.[0] ?? null;
  }

  /** Brings another window forward with its Inbox open at an item (it shows that item's machine). */
  async openInboxIn(label: string, id: string): Promise<void> {
    await this.focus(label).catch(() => {});
    await emitTo(label, "qanuvi-inbox-open", { id }).catch(() => {});
  }

  async focus(label: string): Promise<void> {
    const w = await WebviewWindow.getByLabel(label);
    await w?.unminimize().catch(() => {});
    await w?.setFocus();
  }

  /** Opens a project in a window of its own (or brings forward the window that already shows it). */
  async openProject(projectId: string, name: string): Promise<void> {
    const existing = this.windowOf(projectId);
    if (existing) return this.focus(existing);
    const label = `project-${Date.now().toString(36)}`;
    await this.#open(label, `/?project=${encodeURIComponent(projectId)}`, `${name} · Qanuvi`);
  }

  async #open(label: string, url: string, title: string): Promise<void> {
    const w = new WebviewWindow(label, {
      url,
      title,
      width: 1280,
      height: 820,
      minWidth: 900,
      minHeight: 560,
      center: true,
      decorations: false,
      backgroundColor: "#141416",
      theme: "dark",
    });
    await new Promise<void>((resolve, reject) => {
      w.once("tauri://created", () => resolve());
      w.once("tauri://error", (e) => reject(new Error(String(e.payload))));
    });
  }
}

export const windows = new WindowsStore();
