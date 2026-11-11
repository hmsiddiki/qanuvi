/**
 * Qanuvi's windows (Milestone 15c): one window is the normal way of working; a project can also be opened in a window
 * of its own, for two projects side by side. Windows tell each other which project they show, so a project open in
 * one window is shown as such in the others and opening it again brings that window forward instead.
 */
import { emit, listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow, WebviewWindow } from "@tauri-apps/api/webviewWindow";

interface ProjectNote {
  label: string;
  projectId: string | null;
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
  #current: () => string | null = () => null;

  async init(current: () => string | null): Promise<void> {
    this.#current = current;
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
    // A new window asks the others what they show.
    await listen<{ label: string }>("qanuvi-window-hello", (e) => {
      if (e.payload.label !== this.label) this.announce(this.#current());
    });
    await emit("qanuvi-window-hello", { label: this.label });
    window.addEventListener("beforeunload", () => {
      emit("qanuvi-window-project", { label: this.label, projectId: null });
    });
  }

  announce(projectId: string | null): void {
    emit("qanuvi-window-project", { label: this.label, projectId }).catch(() => {});
  }

  /** The other window showing a project, if any. */
  windowOf(projectId: string): string | null {
    return Object.entries(this.others).find(([, p]) => p === projectId)?.[0] ?? null;
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
    const w = new WebviewWindow(label, {
      url: `/?project=${encodeURIComponent(projectId)}`,
      title: `${name} · Qanuvi`,
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
