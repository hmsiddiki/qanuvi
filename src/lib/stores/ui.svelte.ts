/** What the work pane shows and which overlays are open. */
export type WorkView =
  "editor" | "preview" | "plan" | "search" | "diff" | "agents" | "servers" | "memory" | "account" | "inbox";

class UiStore {
  view = $state<WorkView>("editor");
  quickOpen = $state(false);
  /** Text the "Go to" box opens with ("#" project symbols, "@" file symbols). */
  quickOpenText = $state("");
  /** When set, "Go to file" hands the chosen path here instead of opening it (for example to mention it to the agent). */
  pickFile = $state<((path: string) => void) | null>(null);
  /** Project whose settings dialog is open. */
  settingsFor = $state<string | null>(null);
  /** Goes up to ask the search panel to take focus. */
  focusSearch = $state(0);
  section = $state("home");

  show(view: WorkView): void {
    this.view = view;
  }
}

export const ui = new UiStore();
