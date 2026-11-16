import { open } from "@tauri-apps/plugin-dialog";
import { api, errorText, type Project, type ProjectSettings } from "$lib/api";
import { own, shownMachine } from "$lib/remote";
import { folderPicker } from "./folderPicker.svelte";

/** The project each machine opens with: This PC keeps the old key, a master its own. */
export const currentKey = (machine: string | null) =>
  machine ? `qanuvi.currentProject.${machine}` : "qanuvi.currentProject";

/**
 * The project list and the project being worked on (24z3 Part A2): the list is the shown machine's only, never
 * mixed with another machine's.
 */
class ProjectsStore {
  list = $state<Project[]>([]);
  /** The machine the list belongs to: null is This PC. */
  machine = $state<string | null>(null);
  /** This PC's own project ids, kept while a master is shown (pairing a computer with itself lists the same ones). */
  #localIds = new Set<string>();
  /** Bumped by each load, so an older answer that comes late is dropped. */
  #loads = 0;
  /** The start-up folder (`qanuvi <folder>`) was looked at. */
  #started = false;
  currentId = $state<string | null>(null);
  error = $state("");
  /** The project this window opens with (a project window), instead of the last one used. */
  startWith: string | null = null;
  /** Remember the current project for the next start (the main window only). */
  persist = true;

  get current(): Project | null {
    return this.list.find((p) => p.id === this.currentId) ?? null;
  }

  /** Lists the shown machine's projects (calls without an id go to it, remote.ts). */
  async load(): Promise<void> {
    const machine = shownMachine();
    const load = ++this.#loads;
    this.error = "";
    try {
      const found = await api.projects.list();
      if (load !== this.#loads) return;
      if (!machine) this.#localIds = new Set(found.map((p) => p.id));
      this.machine = machine;
      this.list = machine
        ? found.filter((p) => !this.#localIds.has(p.id)).map((p) => ({ ...p, master: machine }))
        : found;
      const saved = this.startWith ?? localStorage.getItem(currentKey(machine));
      // A project window waits for its project (a server project comes once the server answers) rather than
      // showing another one.
      this.currentId = this.list.some((p) => p.id === saved)
        ? saved
        : this.startWith
          ? null
          : (this.list[0]?.id ?? null);
      // `qanuvi <folder>` opens that folder as the current project (This PC's, once, at start-up).
      const start = this.startWith || machine || this.#started ? null : await api.startupPath();
      this.#started = true;
      if (start) await this.add(start);
    } catch (e) {
      if (load === this.#loads) this.error = errorText(e);
    }
  }

  /** A project this computer has itself (not one of a server's). */
  isLocal(id: string): boolean {
    return this.#localIds.has(id);
  }

  select(id: string): void {
    if (!this.list.some((p) => p.id === id)) return;
    this.currentId = id;
    if (this.persist) localStorage.setItem(currentKey(this.machine), id);
    api.projects.touch(id).catch(() => {});
  }

  /** Picks a folder of the shown machine and adds it: the system picker on This PC, Qanuvi's own on a master. */
  async addWithPicker(): Promise<Project | null> {
    const machine = this.machine;
    if (machine) {
      const path = await folderPicker.pick(machine);
      // Shown machine changed meanwhile: the folder belongs to the one it was picked on, never added elsewhere.
      if (!path || this.machine !== machine) return null;
      return this.add(path);
    }
    const picked = await open({ directory: true, multiple: false, title: "Add a project folder" });
    if (typeof picked !== "string") return null;
    return this.add(picked);
  }

  async add(path: string): Promise<Project | null> {
    this.error = "";
    try {
      const added = await api.projects.add(path);
      const project = this.machine ? { ...added, master: this.machine } : added;
      // Later calls about it go to its machine, whatever is shown then.
      if (this.machine) own(project.id, this.machine);
      else this.#localIds.add(project.id);
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
      if (!this.machine) this.#localIds.delete(id);
      if (this.currentId === id) {
        this.currentId = this.list[0]?.id ?? null;
        const key = currentKey(this.machine);
        if (this.currentId) localStorage.setItem(key, this.currentId);
        else localStorage.removeItem(key);
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
