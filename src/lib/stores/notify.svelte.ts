/**
 * Desktop notifications (24u part 4, session B): the window's side. It tells Qanuvi the owner's settings (the
 * desktop part decides and shows), and a clicked notification opens the Inbox at its item: in the window it was sent
 * to, or (when the click started Qanuvi) once this window is ready.
 */
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { settings } from "./settings.svelte";
import { ui } from "./ui.svelte";

type Open = { id: string | null };

/** Opens the Inbox drop-down, at the item when there is one. */
export function openInbox(id: string | null): void {
  ui.closeSettings();
  ui.inboxFocus = id;
  ui.show("inbox");
}

let started = false;

export async function startNotifications(): Promise<void> {
  if (started) return;
  started = true;
  $effect.root(() => {
    $effect(() => {
      const s = $state.snapshot(settings.value.notifications);
      void invoke("notify_settings", { settings: s }).catch(() => {});
    });
  });
  await getCurrentWebviewWindow().listen<Open>("qanuvi-inbox-open", (e) => openInbox(e.payload?.id ?? null));
  const first = await invoke<Open | null>("notify_take_start").catch(() => null);
  if (first) openInbox(first.id);
}

/** Sends a test notification (Settings > Notifications). */
export function testNotification(): Promise<void> {
  return invoke("notify_test");
}
