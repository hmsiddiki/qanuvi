import { open } from "@tauri-apps/plugin-dialog";
import { api, errorText, type Project, type ProjectSettings } from "$lib/api";

const CURRENT_KEY = "qanuvi.currentProject";

/** The project list and the project being worked on. */
class ProjectsStore {
  list = $state<Project[]>([]);
  currentId = $state<string | null>(null);
  error = $state("");
  /** The project this window opens with (a project window), instead of the last one used. */
  startWith: string | null = null;
  /** Remember the current project for the next start (the main window only). */
  persist = true;

  get current(): Project | null {
    return this.list.find((p) => p.id === this.currentId) ?? null;
  }

  async load(): Promise<void> {
    try {
      const local = await api.projects.list();
      this.list = [...local, ...this.list.filter((p) => p.master)];
      const saved = this.startWith ?? localStorage.getItem(CURRENT_KEY);
      // A project window waits for its project (a server project comes once the server answers) rather than
      // showing another one.
      this.currentId = this.list.some((p) => p.id === saved)
        ? saved
        : this.startWith
          ? null
          : (this.list[0]?.id ?? null);
      // `qanuvi <folder>` opens that folder as the current project.
      const start = this.startWith ? null : await api.startupPath();
      if (start) await this.add(start);
    } catch (e) {
      this.error = errorText(e);
    }
  }

  /** Adds (or refreshes) projects of a server; a project the window was on last time is opened again. */
  addRemote(master: string, list: Project[]): void {
    // A project that is on this computer stays this computer's (pairing a computer with itself lists the same ones).
    const tagged = list.filter((p) => !this.isLocal(p.id)).map((p) => ({ ...p, master }));
    const ids = new Set(tagged.map((p) => p.id));
    this.list = [...this.list.filter((p) => !ids.has(p.id)), ...tagged];
    const saved = this.startWith ?? localStorage.getItem(CURRENT_KEY);
    if (saved && ids.has(saved) && this.currentId !== saved) this.currentId = saved;
  }

  /** A project this computer has itself (not one of a server's). */
  isLocal(id: string): boolean {
    return this.list.some((p) => p.id === id && !p.master);
  }

  /** Takes a server's projects out of the list (its connection ended). */
  dropRemote(master: string): void {
    this.list = this.list.filter((p) => p.master !== master);
    if (this.currentId && !this.list.some((p) => p.id === this.currentId)) this.currentId = this.list[0]?.id ?? null;
  }

  select(id: string): void {
    if (!this.list.some((p) => p.id === id)) return;
    this.currentId = id;
    if (this.persist) localStorage.setItem(CURRENT_KEY, id);
    api.projects.touch(id).catch(() => {});
  }

  /** Opens the system folder picker and adds the chosen folder. */
  async addWithPicker(): Promise<Project | null> {
    const picked = await open({ directory: true, multiple: false, title: "Add a project folder" });
    if (typeof picked !== "string") return null;
    return this.add(picked);
  }

  async add(path: string): Promise<Project | null> {
    this.error = "";
    try {
      const project = await api.projects.add(path);
      if (!this.list.some((p) => p.id === project.id)) this.list = [project, ...this.list];
      this.select(project.id);
      return project;
    } catch (e) {
      this.error = errorText(e);
      return null;
    }
  }

  async remove(id: string): Promise<void> {
    try {
      await api.projects.remove(id);
      this.list = this.list.filter((p) => p.id !== id);
      if (this.currentId === id) {
        this.currentId = this.list[0]?.id ?? null;
        if (this.currentId) localStorage.setItem(CURRENT_KEY, this.currentId);
        else localStorage.removeItem(CURRENT_KEY);
      }
    } catch (e) {
      this.error = errorText(e);
    }
  }

  async update(id: string, name: string, s: ProjectSettings): Promise<string> {
    try {
      const updated = await api.projects.update(id, name, s);
      this.list = this.list.map((p) => (p.id === id ? { ...updated, master: p.master } : p));
      return "";
    } catch (e) {
      return errorText(e);
    }
  }

  async redetect(id: string): Promise<void> {
    try {
      const updated = await api.projects.redetect(id);
      this.list = this.list.map((p) => (p.id === id ? { ...updated, master: p.master } : p));
    } catch (e) {
      this.error = errorText(e);
    }
  }
}

export const projects = new ProjectsStore();

/** Colour and short label for each project type. */
export const KIND_STYLE: Record<string, { label: string; color: string }> = {
  laravel: { label: "Laravel", color: "#f05340" },
  qlopy: { label: "Qlopy", color: "#4f8cff" },
  php: { label: "PHP", color: "#8892bf" },
  flutter: { label: "Flutter", color: "#42a5f5" },
  android: { label: "Android", color: "#3ddc84" },
  dotnet: { label: ".NET", color: "#a179dc" },
  python: { label: "Python", color: "#e5c14b" },
  node: { label: "Node", color: "#6cc24a" },
  rust: { label: "Rust", color: "#dea584" },
  folder: { label: "Folder", color: "#9b9ba3" },
};

export function kindStyle(kind: string) {
  return KIND_STYLE[kind] ?? KIND_STYLE.folder;
}
