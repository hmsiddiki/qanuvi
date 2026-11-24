/**
 * t1: the tray's "Quit Qanuvi" question, asked in this window's own dialog (the master has no window of its own).
 * The master brings this window forward and sends the question; the answer goes back to it. A question already shown
 * is not asked twice (Quit again only brings it forward). s10: with plan runs going it also offers "Go on next time"
 * (the runs go on by themselves at the next start), as the main button.
 */
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { choose, type Answer } from "$lib/stores/confirm.svelte";

export interface QuitQuestion {
  id: number;
  text: string;
  later?: boolean;
}

/** The dialog's words, the same in the window and in the question window. */
export const quitWords = {
  title: "Quit Qanuvi",
  kind: "warning" as const,
  okLabel: "Quit",
  cancelLabel: "Keep running",
  laterLabel: "Go on next time",
};

/** The buttons for a question: with "Go on next time" offered it is the main one and Quit is the middle one. */
export function quitButtons(q: QuitQuestion) {
  return q.later
    ? { ...quitWords, okLabel: quitWords.laterLabel, otherLabel: quitWords.okLabel }
    : { ...quitWords, otherLabel: "" };
}

/** What a button press answers the master: {yes, later}. */
export function quitAnswer(q: QuitQuestion, a: Answer): { yes: boolean; later: boolean } {
  if (q.later) return { yes: a !== false, later: a === true };
  return { yes: a === true, later: false };
}

const shown = new Set<number>();

export async function startQuitQuestion(): Promise<void> {
  await getCurrentWebviewWindow().listen<QuitQuestion>("qanuvi-quit-question", async (e) => {
    const q = e.payload;
    if (!q || shown.has(q.id)) return;
    shown.add(q.id);
    const a = await choose(q.text, quitButtons(q));
    await invoke("quit_question_answer", { id: q.id, ...quitAnswer(q, a) }).catch(() => {});
  });
}
