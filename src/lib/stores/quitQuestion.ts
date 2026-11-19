/**
 * t1: the tray's "Quit Qanuvi" question, asked in this window's own dialog (the master has no window of its own).
 * The master brings this window forward and sends the question; the answer goes back to it. A question already shown
 * is not asked twice (Quit again only brings it forward).
 */
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { ask } from "$lib/stores/confirm.svelte";

interface Question {
  id: number;
  text: string;
}

/** The dialog's words, the same in the window and in the question window. */
export const quitWords = {
  title: "Quit Qanuvi",
  kind: "warning" as const,
  okLabel: "Quit",
  cancelLabel: "Keep running",
};

const shown = new Set<number>();

export async function startQuitQuestion(): Promise<void> {
  await getCurrentWebviewWindow().listen<Question>("qanuvi-quit-question", async (e) => {
    const q = e.payload;
    if (!q || shown.has(q.id)) return;
    shown.add(q.id);
    const yes = await ask(q.text, quitWords);
    await invoke("quit_question_answer", { id: q.id, yes }).catch(() => {});
  });
}
