import { panel } from "./panel.svelte";

/** What the work pane shows and which overlays are open. */
export type WorkView = "editor" | "preview" | "plan" | "search" | "diff" | "inbox";

/** Where a link can lead: a view of the work area, or a part of the settings page. */
export type Place = WorkView | "agents" | "servers" | "memory" | "account";

/** The parts of the settings page (15h). */
export type SettingsTab =
  | "agents"
  | "models"
  | "team"
  | "servers"
  | "memory"
  | "backups"
  | "account"
  | "layout"
  | "appearance"
  | "languages"
  | "startup"
  | "notifications"
  | "updates";

/** Views that used to be pages in the work area: they open the settings page now. */
const SETTINGS_VIEWS: Record<string, SettingsTab> = {
  agents: "agents",
  servers: "servers",
  memory: "memory",
  account: "account",
};

class UiStore {
  view = $state<WorkView>("editor");
  quickOpen = $state(false);
  /** Text the "Go to" box opens with ("#" project symbols, "@" file symbols). */
  quickOpenText = $state("");
  /** When set, "Go to file" hands the chosen path here instead of opening it (for example to mention it to the agent). */
  pickFile = $state<((path: string) => void) | null>(null);
  /** Project whose settings dialog is open. */
  settingsFor = $state<string | null>(null);
  section = $state("home");

  /** The settings page, open at this part; null: closed. */
  settings = $state<SettingsTab | null>(null);

  /** Shows a view of the work area; the former settings views open the settings page instead. */
  show(view: Place): void {
    const tab = SETTINGS_VIEWS[view];
    if (tab) {
      this.openSettings(tab);
      return;
    }
    if (view === "inbox") {
      this.inboxOpen = true;
      return;
    }
    this.settings = null;
    if (view === "search") {
      // Search lives in the project panel now.
      panel.show("search");
      panel.focusSearch++;
      this.searchWanted++;
      return;
    }
    this.view = view as WorkView;
  }

  /** The Inbox drop-down from the sidebar. */
  inboxOpen = $state(false);
  /** The Inbox item a clicked desktop notification opened (shown and marked for a moment). */
  inboxFocus = $state<string | null>(null);
  /** Goes up when search was asked for: the work area shows the project panel. */
  searchWanted = $state(0);

  openSettings(tab: SettingsTab = "agents"): void {
    this.settings = tab;
  }

  closeSettings(): void {
    this.settings = null;
  }
}

export const ui = new UiStore();
