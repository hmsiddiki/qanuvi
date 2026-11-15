/**
 * Desktop notifications (24u part 4, session B): the window's side. It tells Qanuvi the owner's settings (the
 * desktop part decides and shows), and a clicked notification opens the Inbox at its item: in the window it was sent
 * to, or (when the click started Qanuvi) once this window is ready. Each machine has its own Notifications settings
 * (24z3 D2): This PC's go to the desktop part; a master's are kept here and sent with each of its new items.
 */
import { invoke } from "@tauri-apps/api/core";
import { callMaster } from "$lib/remote";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { parseNotifications, settings, type NotifySettings } from "./settings.svelte";
import { ui } from "./ui.svelte";

type Open = { id: string | null };

/** Opens the Inbox drop-down, at the item when there is one. */
export function openInbox(id: string | null): void {
  ui.closeSettings();
  ui.inboxFocus = id;
  ui.show("inbox");
}

/** Each connected master's Notifications settings (read on connect, kept current while it is shown). */
const ofMaster = new Map<string, NotifySettings>();

/** A master's Notifications settings, for its new Inbox items: the defaults until read (an older master has none). */
export function notifyFor(master: string): NotifySettings {
  return ofMaster.get(master) ?? parseNotifications(undefined);
}

/** Reads a master's Notifications settings (inbox.svelte.ts, when it starts following the master). */
export async function learnNotifications(master: string): Promise<void> {
  try {
    const kept = await callMaster<Record<string, unknown>>(master, "settings_get", {});
    // While it is shown, its Settings page keeps the value current.
    if (settings.machine !== master) ofMaster.set(master, parseNotifications(kept.notifications));
  } catch {
    // An older master keeps no settings: the defaults.
  }
}

export function forgetNotifications(master: string): void {
  ofMaster.delete(master);
}

let started = false;

export async function startNotifications(): Promise<void> {
  if (started) return;
  started = true;
  $effect.root(() => {
    $effect(() => {
      const s = $state.snapshot(settings.value.notifications);
      const machine = settings.machine;
      if (machine) ofMaster.set(machine, s);
      else void invoke("notify_settings", { settings: s }).catch(() => {});
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
