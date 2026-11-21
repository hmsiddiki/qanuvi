import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { AppInfo } from "./platform";

/** LT: this window's choice of the system's title bar on Linux (kept with the window, like its layout). */
const SYSTEM_TITLE_BAR_KEY = "qanuvi.systemTitleBar";

function savedSystemTitleBar(): boolean {
  try {
    return localStorage.getItem(SYSTEM_TITLE_BAR_KEY) === "true";
  } catch {
    return false;
  }
}

/** Facts about the running app, read once from the Rust core at start. */
class PlatformStore {
  info = $state<AppInfo | null>(null);
  /** LT: Linux only: the system draws the title bar and window buttons (off: Qanuvi draws them, as on Windows). */
  systemTitleBar = $state(savedSystemTitleBar());

  get os(): string {
    return this.info?.os ?? "";
  }

  get isMac(): boolean {
    return this.os === "macos";
  }

  /** Qanuvi draws its own window buttons on Windows, and on Linux unless the system's title bar is chosen (LT);
   * macOS keeps the system's. */
  get drawsWindowControls(): boolean {
    return this.os === "windows" || (this.os === "linux" && !this.systemTitleBar);
  }

  /** LT: Linux: uses the system's title bar or Qanuvi's own, at once and from now on. */
  async setSystemTitleBar(on: boolean): Promise<void> {
    this.systemTitleBar = on;
    try {
      localStorage.setItem(SYSTEM_TITLE_BAR_KEY, String(on));
    } catch {
      // Not kept: it holds for this run.
    }
    await getCurrentWindow()
      .setDecorations(on)
      .catch(() => {});
  }

  async load(): Promise<void> {
    try {
      this.info = await invoke<AppInfo>("app_info");
      // LT: Linux windows open without the system's bar (tauri.linux.conf.json); it is put back when chosen.
      if (this.os === "linux" && this.systemTitleBar)
        await getCurrentWindow()
          .setDecorations(true)
          .catch(() => {});
    } catch {
      this.info = null;
    }
  }
}

export const platform = new PlatformStore();
