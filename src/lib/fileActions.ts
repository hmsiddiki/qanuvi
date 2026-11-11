/**
 * What the right-click menus do with files (15d): shared by the file tree, the editor tabs, the editor and the Git
 * view. Paths are project-relative with "/".
 */
import { ask } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { api, errorText, type Project } from "$lib/api";
import { contextMenu, copyText } from "$lib/stores/contextMenu.svelte";
import { editor } from "$lib/stores/editor.svelte";
import { agent } from "$lib/stores/agent.svelte";
import { terminals } from "$lib/stores/terminals.svelte";
import { layout } from "$lib/layout/layout.svelte";
import { gitState } from "$lib/stores/git.svelte";

/** The folder a path is in ("" for the project's top). */
export const parentOf = (rel: string) => (rel.includes("/") ? rel.slice(0, rel.lastIndexOf("/")) : "");
export const nameOf = (rel: string) => rel.split("/").pop() ?? rel;
const join = (dir: string, name: string) => (dir ? `${dir}/${name}` : name).replace(/\/+/g, "/");

/** The full path on disk, with the system's separator. */
export function absolutePath(project: Project, rel: string): string {
  const sep = project.path.includes("\\") ? "\\" : "/";
  const root = project.path.replace(/[\\/]+$/, "");
  return rel ? `${root}${sep}${rel.split("/").join(sep)}` : root;
}

/** Shows a problem to the user (actions run from a menu have no other place to say it). */
async function report(what: string, e: unknown) {
  const { message } = await import("@tauri-apps/plugin-dialog");
  await message(`${what}: ${errorText(e)}`, { title: "Qanuvi", kind: "error" }).catch(() => {});
}

export const copyPath = (project: Project, rel: string) => copyText(absolutePath(project, rel));
export const copyRelativePath = (rel: string) => copyText(rel);

export async function reveal(project: Project, rel: string) {
  try {
    await revealItemInDir(absolutePath(project, rel));
  } catch (e) {
    await report("It could not be shown in the file manager", e);
  }
}

/** Opens a terminal in a folder of the project. */
export function openTerminal(project: Project, dir: string) {
  if (!layout.state.workVisible) layout.toggleWork();
  if (!layout.state.terminalVisible) layout.toggleTerminal();
  terminals.open(project.id, undefined, dir || undefined);
}

/** Puts a reference to the file (and lines) into the message box of the conversation. */
export function addToMessage(rel: string, lines?: { from: number; to: number; text: string }) {
  if (!layout.state.agentVisible) layout.toggleAgent();
  const ref = lines ? `@${rel}:${lines.from}${lines.to > lines.from ? `-${lines.to}` : ""}` : `@${rel}`;
  const block = lines?.text ? `${ref}\n\`\`\`\n${lines.text}\n\`\`\`\n` : `${ref} `;
  agent.draft = agent.draft && !/\s$/.test(agent.draft) ? `${agent.draft}\n${block}` : agent.draft + block;
  queueMicrotask(() => agent.input?.focus());
}

/** Asks for a name and makes a new file or folder in \`dir\`; files open in the editor. */
export function newItem(project: Project, dir: string, folder: boolean, after?: () => unknown) {
  contextMenu.ask(
    folder ? `New folder in ${dir || project.name}` : `New file in ${dir || project.name}`,
    "",
    "Create",
    async (name) => {
      const path = join(dir, name);
      try {
        await api.files.create(project.id, path, folder);
        gitState.changed++;
        await after?.();
        if (!folder) await editor.open(project.id, path);
      } catch (e) {
        await report(`${path} could not be made`, e);
      }
    },
  );
}

/** Asks for a new name and renames; open tabs of the file (or of files in the folder) follow it. */
export function rename(project: Project, rel: string, after?: () => unknown) {
  contextMenu.ask(`Rename ${nameOf(rel)}`, nameOf(rel), "Rename", async (name) => {
    const to = join(parentOf(rel), name);
    if (to === rel) return;
    try {
      await api.files.rename(project.id, rel, to);
      gitState.changed++;
      for (const tab of editor.tabs.filter(
        (t) => t.projectId === project.id && (t.path === rel || t.path.startsWith(`${rel}/`)),
      )) {
        const moved = to + tab.path.slice(rel.length);
        const wasActive = editor.activeKey === tab.key;
        if (tab.dirty) continue; // Unsaved work stays where it is; the user decides.
        await editor.close(tab.key);
        if (wasActive) await editor.open(project.id, moved);
      }
      await after?.();
    } catch (e) {
      await report(`${rel} could not be renamed`, e);
    }
  });
}

/** Moves a file or folder to the Recycle Bin after asking; its open tabs close. */
export async function trash(project: Project, rel: string, folder: boolean, after?: () => unknown) {
  const ok = await ask(`Move ${folder ? "the folder" : ""} ${rel} to the Recycle Bin? It can be restored from there.`, {
    title: "Delete",
    kind: "warning",
    okLabel: "Move to Recycle Bin",
    cancelLabel: "Keep it",
  });
  if (!ok) return;
  try {
    await api.files.trash(project.id, rel);
    gitState.changed++;
    for (const tab of editor.tabs.filter(
      (t) => t.projectId === project.id && (t.path === rel || t.path.startsWith(`${rel}/`)),
    )) {
      tab.dirty = false;
      await editor.close(tab.key);
    }
    await after?.();
  } catch (e) {
    await report(`${rel} could not be deleted`, e);
  }
}

/** Git: the file's changes go into the next commit. */
export async function stage(project: Project, rel: string) {
  try {
    await api.git.stage(project.id, rel);
    gitState.changed++;
  } catch (e) {
    await report(`${rel} could not be staged`, e);
  }
}

/** Git: the file goes back to how it is in the last commit, after asking. */
export async function discard(project: Project, rel: string) {
  const ok = await ask(`Discard the changes to ${rel}? They cannot be brought back.`, {
    title: "Discard changes",
    kind: "warning",
    okLabel: "Discard",
    cancelLabel: "Keep them",
  });
  if (!ok) return;
  try {
    await api.git.revert(project.id, rel);
    gitState.changed++;
    const tab = editor.tabs.find((t) => t.projectId === project.id && t.path === rel);
    if (tab && !tab.dirty) await editor.reload(tab.key);
  } catch (e) {
    await report(`${rel} could not be reset`, e);
  }
}
