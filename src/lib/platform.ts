/** What the Rust core reports about the running app (see `AppInfo` in src-tauri/src/lib.rs). */
export interface AppInfo {
  name: string;
  version: string;
  os: string;
  arch: string;
}

const OS_LABELS: Record<string, string> = {
  windows: "Windows",
  linux: "Linux",
  macos: "macOS",
  android: "Android",
  ios: "iOS",
};

/** A short, readable platform label such as "Windows x86_64". Unknown systems are shown as reported. */
export function platformLabel(info: Pick<AppInfo, "os" | "arch">): string {
  const os = OS_LABELS[info.os] ?? info.os;
  return info.arch ? `${os} ${info.arch}` : os;
}
