/** Qanuvi's total memory and processor use (itself and every process it started), for the status bar (1f). */
import { invoke } from "@tauri-apps/api/core";

export interface UsagePart {
  name: string;
  count: number;
  memory: number;
}

export interface Usage {
  memory: number;
  cpu: number;
  processes: number;
  parts: UsagePart[];
}

/** Memory in people's units. */
export function bytes(n: number): string {
  if (n >= 1024 ** 3) return `${(n / 1024 ** 3).toFixed(1)} GB`;
  return `${Math.round(n / 1024 ** 2)} MB`;
}

class UsageStore {
  now = $state<Usage | null>(null);
  #timer: ReturnType<typeof setInterval> | null = null;

  start(): void {
    if (this.#timer) return;
    const look = () =>
      invoke<Usage | null>("qanuvi_usage")
        .then((u) => (this.now = u))
        .catch(() => {});
    void look();
    this.#timer = setInterval(look, 5000);
  }
}

export const usage = new UsageStore();
