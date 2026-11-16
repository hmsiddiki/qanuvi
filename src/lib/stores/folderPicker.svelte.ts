/**
 * The folder picker for a server's folders (24z3 step 3, f1): the system picker only sees this PC, so a shown master
 * gets Qanuvi's own, reading that machine's folders (dirs_list). Every call names the machine, so a switch meanwhile
 * never reads another one's; the switch itself closes the picker.
 */
import { errorText, type Folders } from "$lib/api";
import { callOn, machineName } from "$lib/remote";

interface Open {
  machine: string;
  name: string;
  resolve: (path: string | null) => void;
}

class FolderPickerStore {
  current = $state<Open | null>(null);
  /** The folder listed now (null until the first answer). */
  folder = $state<Folders | null>(null);
  /** The path box: the listed folder's path, or what the owner typed. */
  typed = $state("");
  error = $state("");
  loading = $state(false);
  #reads = 0;

  /** Opens the picker on a machine, at its home folder. Answers the folder chosen, or null. */
  pick(machine: string): Promise<string | null> {
    this.cancel();
    return new Promise((resolve) => {
      this.current = { machine, name: machineName(machine), resolve };
      this.folder = null;
      this.typed = "";
      this.error = "";
      void this.go("");
    });
  }

  /** Lists a folder of the picker's machine. A late answer (another folder asked since) is dropped. */
  async go(path: string): Promise<void> {
    const c = this.current;
    if (!c) return;
    const read = ++this.#reads;
    this.loading = true;
    try {
      const found = await callOn<Folders>(c.machine, "dirs_list", { path });
      if (read !== this.#reads || this.current !== c) return;
      this.folder = found;
      this.typed = found.path;
      this.error = "";
    } catch (e) {
      if (read !== this.#reads || this.current !== c) return;
      // The folder listed before stays, so the owner can fix the path or go on from there.
      this.error = errorText(e);
    } finally {
      if (read === this.#reads) this.loading = false;
    }
  }

  up(): void {
    if (this.folder?.parent) void this.go(this.folder.parent);
  }

  /** The folder in the path box: a typed one is checked by the machine first (it must exist). */
  async choose(): Promise<void> {
    const c = this.current;
    if (!c) return;
    const want = this.typed.trim();
    if (want && want !== this.folder?.path) {
      await this.go(want);
      if (this.current !== c || this.error) return;
    }
    const path = this.folder?.path;
    if (!path) return;
    this.#close(path);
  }

  cancel(): void {
    this.#close(null);
  }

  /** The shown machine changed: a picker for another machine closes. */
  switched(shown: string | null): void {
    if (this.current && this.current.machine !== shown) this.cancel();
  }

  #close(path: string | null): void {
    const c = this.current;
    if (!c) return;
    this.current = null;
    this.folder = null;
    this.#reads++;
    this.loading = false;
    c.resolve(path);
  }
}

export const folderPicker = new FolderPickerStore();
