import { invoke } from "@tauri-apps/api/core";
import type { AppInfo } from "./platform";

/** Facts about the running app, read once from the Rust core at start. */
class PlatformStore {
  info = $state<AppInfo | null>(null);

  get os(): string {
    return this.info?.os ?? "";
  }

  get isMac(): boolean {
    return this.os === "macos";
  }

  /** On Windows Qanuvi draws its own window buttons; macOS and Linux keep the system's. */
  get drawsWindowControls(): boolean {
    return this.os === "windows";
  }

  async load(): Promise<void> {
    try {
      this.info = await invoke<AppInfo>("app_info");
    } catch {
      this.info = null;
    }
  }
}

export const platform = new PlatformStore();
