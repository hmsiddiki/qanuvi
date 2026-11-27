/**
 * s3 (ST2): follows the shown project's folder on its machine, so files made or changed outside Qanuvi (a terminal,
 * git, other tools, an agent) show at once in the tree, the Git list and open tabs, as in VS Code. Followed again
 * when the machine's link comes back; an older master without the watch keeps the 15-second Git refresh.
 */
import { Channel } from "@tauri-apps/api/core";
import { api, type FileChanges } from "$lib/api";
import { onLinkEnd, shownMachine } from "$lib/remote";
import { editor } from "./editor.svelte";
import { gitState } from "./git.svelte";

/** A build can write files all the time: the Git list is asked again at most this often. */
const GIT_EVERY_MS = 2000;
/** Tries to follow again after the link ended, this far apart. */
const RETRY_MS = 3000;
const RETRIES = 20;

class FileWatchStore {
  /** Goes up after each note of changes (the tree reloads what it shows). */
  changed = $state(0);
  /** The paths of the last note (relative, with "/"); `all`: too many to list. */
  last = $state<{ projectId: string; paths: string[]; all: boolean }>({ projectId: "", paths: [], all: false });

  #key = "";
  #stop: (() => void) | null = null;
  #gitAt = 0;
  #gitTimer: ReturnType<typeof setTimeout> | null = null;

  /** Follows this project of the shown machine (once; a new project or machine replaces the one before). */
  follow(projectId: string): void {
    const machine = shownMachine();
    const key = `${machine ?? ""}|${projectId}`;
    if (key === this.#key) return;
    this.#key = key;
    this.#stop?.();
    this.#stop = null;
    void this.#start(machine, projectId, key, 0);
  }

  async #start(machine: string | null, projectId: string, key: string, tries: number): Promise<void> {
    const events = new Channel<FileChanges>();
    events.onmessage = (n) => {
      if (this.#key !== key || n.ping) return;
      const paths = n.paths ?? [];
      this.last = { projectId, paths, all: !!n.more };
      this.changed++;
      void editor.diskChanged(projectId, paths, !!n.more);
      if (n.git || paths.length || n.more) this.#touchGit();
    };
    try {
      await api.files.watch(projectId, events);
    } catch {
      // The link is not back yet: again in a moment; an older master refuses it every time (then it stops).
      if (this.#key === key && tries < RETRIES)
        setTimeout(() => this.#retry(machine, projectId, key, tries + 1), RETRY_MS);
      return;
    }
    if (this.#key !== key) return;
    this.#stop = await onLinkEnd(machine, () => {
      if (this.#key !== key) return;
      this.#stop?.();
      this.#stop = null;
      setTimeout(() => this.#retry(machine, projectId, key, 0), RETRY_MS);
    });
  }

  #retry(machine: string | null, projectId: string, key: string, tries: number): void {
    if (this.#key === key) void this.#start(machine, projectId, key, tries);
  }

  #touchGit(): void {
    const wait = this.#gitAt + GIT_EVERY_MS - Date.now();
    if (wait <= 0) {
      this.#gitAt = Date.now();
      gitState.touch();
    } else if (!this.#gitTimer) {
      this.#gitTimer = setTimeout(() => {
        this.#gitTimer = null;
        this.#gitAt = Date.now();
        gitState.touch();
      }, wait);
    }
  }
}

export const fileWatch = new FileWatchStore();
