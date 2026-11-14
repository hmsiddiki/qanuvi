/**
 * Starting and steering debugging from anywhere (the sidebar's buttons and the keys VS Code uses): F5 starts or
 * continues, Shift+F5 stops, Ctrl+Shift+F5 restarts, F6 pauses, F10 steps over, F11 steps in, Shift+F11 steps out.
 */
import { layout } from "$lib/layout/layout.svelte";
import { dap } from "$lib/stores/dap.svelte";
import { lsp } from "$lib/stores/lsp.svelte";
import { panel } from "$lib/stores/panel.svelte";
import { projects } from "$lib/stores/projects.svelte";

/** Shows the debug console below the editor. */
export function showDebugConsole(): void {
  lsp.bottom = "debug";
  if (!layout.state.terminalVisible) layout.toggleTerminal();
}

/** Starts the picked configuration of the current project. */
export async function startDebugging(): Promise<void> {
  const p = projects.current;
  if (!p) return;
  await dap.loadConfigs(p.id);
  const config = dap.pickedConfig(p.id);
  if (!config) return;
  if (panel.view !== "debug") panel.show("debug");
  showDebugConsole();
  await dap.start(p.id, config);
}

/** The debugging keys; true when the key was one of them. */
export function debugKey(e: KeyboardEvent): boolean {
  if (e.altKey || e.metaKey) return false;
  const ctrl = e.ctrlKey;
  const shift = e.shiftKey;
  const s = dap.session;
  const paused = s?.status === "stopped";
  switch (e.key) {
    case "F5":
      if (ctrl && shift && s) dap.restart();
      else if (shift && !ctrl && s) dap.stop();
      else if (!ctrl && !shift) {
        if (paused) dap.continue();
        else if (!s) startDebugging();
        else return true;
      } else return false;
      return true;
    case "F6":
      if (s && !paused && !ctrl && !shift) dap.pause();
      return !!s;
    case "F10":
      if (paused && !ctrl && !shift) dap.stepOver();
      return !!s;
    case "F11":
      if (paused && !ctrl) shift ? dap.stepOut() : dap.stepIn();
      return !!s;
  }
  return false;
}
