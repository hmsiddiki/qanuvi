/**
 * Debugging helpers (15i part 3) that need no window: VS Code's launch.json read as it is (comments and trailing
 * commas allowed, its ${...} variables filled in), which debugger serves a configuration, and file paths between the
 * project's relative form and the full form debuggers use.
 */

/** One configuration of launch.json, as written there. */
export interface LaunchConfig {
  name: string;
  type: string;
  request: "launch" | "attach";
  [key: string]: unknown;
}

/** A breakpoint on a line (1-based), with an optional condition or log message. */
export interface Breakpoint {
  line: number;
  condition?: string;
  logMessage?: string;
  enabled: boolean;
}

/** Which of Qanuvi's debuggers serves each launch.json `type`. */
const ADAPTERS: Record<string, string> = {
  node: "node",
  "pwa-node": "node",
  "node-terminal": "node",
  php: "php",
};

/** PHP on this computer and the Xdebug a debug run gets (from the Rust side's check). */
export interface PhpInfo {
  php: string;
  version: string;
  build: string;
  xdebug: string;
  /** "ini": PHP's settings load Xdebug; "file": Qanuvi loads the chosen file for debug runs only; "": none. */
  via: string;
  note: string;
  /** The Xdebug file chosen in Settings (added by the window). */
  xdebugFile?: string;
}

const RENAMED = /^(?:PHP Warning:\s*)?Xdebug: \[Config\] The setting '([^']+)' has been renamed\b.*$/;

/**
 * Takes Xdebug 3's "setting has been renamed" warnings (old Xdebug 2 keys left in php.ini) out of PHP's output;
 * answers the rest and the old keys, so the console can say it once.
 */
export function foldRenamed(text: string): { text: string; renamed: string[] } {
  const renamed: string[] = [];
  const kept = text.split(/\r?\n/).filter((line) => {
    const m = RENAMED.exec(line.trim());
    if (m) renamed.push(m[1]);
    return !m;
  });
  return { text: kept.join("\n"), renamed };
}

export function adapterFor(type: string): string | null {
  return ADAPTERS[type] ?? null;
}

/** JSON with comments and trailing commas (launch.json, like VS Code's other settings files) to plain JSON. */
export function stripJsonc(text: string): string {
  let out = "";
  let i = 0;
  while (i < text.length) {
    const c = text[i];
    if (c === '"') {
      let j = i + 1;
      while (j < text.length && text[j] !== '"') j += text[j] === "\\" ? 2 : 1;
      out += text.slice(i, j + 1);
      i = j + 1;
    } else if (c === "/" && text[i + 1] === "/") {
      while (i < text.length && text[i] !== "\n") i++;
    } else if (c === "/" && text[i + 1] === "*") {
      const end = text.indexOf("*/", i + 2);
      i = end < 0 ? text.length : end + 2;
    } else {
      out += c;
      i++;
    }
  }
  return removeTrailingCommas(out);
}

function removeTrailingCommas(text: string): string {
  let out = "";
  let i = 0;
  while (i < text.length) {
    const c = text[i];
    if (c === '"') {
      let j = i + 1;
      while (j < text.length && text[j] !== '"') j += text[j] === "\\" ? 2 : 1;
      out += text.slice(i, j + 1);
      i = j + 1;
      continue;
    }
    if (c === ",") {
      let j = i + 1;
      while (j < text.length && /\s/.test(text[j])) j++;
      if (text[j] === "}" || text[j] === "]") {
        i++;
        continue;
      }
    }
    out += c;
    i++;
  }
  return out;
}

/** Reads launch.json: its configurations, or why it cannot be used. */
export function readLaunch(text: string): { configs: LaunchConfig[]; error: string } {
  let raw: unknown;
  try {
    raw = JSON.parse(stripJsonc(text));
  } catch (e) {
    return { configs: [], error: `.vscode/launch.json is not valid JSON: ${e instanceof Error ? e.message : e}` };
  }
  const list = (raw as { configurations?: unknown })?.configurations;
  if (!Array.isArray(list)) return { configs: [], error: '.vscode/launch.json has no "configurations" list.' };
  const configs = list.filter(
    (c): c is LaunchConfig =>
      !!c &&
      typeof c === "object" &&
      typeof (c as LaunchConfig).name === "string" &&
      typeof (c as LaunchConfig).type === "string" &&
      ((c as LaunchConfig).request === "launch" || (c as LaunchConfig).request === "attach"),
  );
  return { configs, error: "" };
}

/** What ${...} variables are filled in with. */
export interface VarContext {
  /** The project's folder (full path, as this computer writes it). */
  root: string;
  /** The shown file, relative to the project ("" when none). */
  file: string;
  line: number;
  selectedText: string;
  env: Record<string, string>;
  home: string;
}

export function separator(root: string): string {
  return /^[A-Za-z]:/.test(root) || root.includes("\\") ? "\\" : "/";
}

/** A project-relative path (forward slashes) as a full path. */
export function absPath(root: string, relative: string): string {
  const sep = separator(root);
  const base = root.replace(/[\\/]+$/, "");
  if (!relative) return base;
  return base + sep + relative.split("/").join(sep);
}

/** A full path as a project-relative one (forward slashes), or null when it is outside the project. */
export function relPath(root: string, full: string): string | null {
  const norm = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "");
  const windows = /^[A-Za-z]:/.test(root);
  let a = norm(root);
  let b = norm(full.replace(/^file:\/\/\/?/i, ""));
  if (windows) b = b.replace(/^\/([A-Za-z]:)/, "$1");
  const key = (p: string) => (windows ? p.toLowerCase() : p);
  if (!key(b).startsWith(`${key(a)}/`)) return null;
  a = b.slice(a.length + 1);
  return a;
}

/** Fills VS Code's ${...} variables in a configuration. Answers the variables Qanuvi cannot fill (input, command). */
export function substitute(config: LaunchConfig, ctx: VarContext): { config: LaunchConfig; unknown: string[] } {
  const unknown = new Set<string>();
  const sep = separator(ctx.root);
  const fileAbs = ctx.file ? absPath(ctx.root, ctx.file) : "";
  const base = fileAbs.split(/[\\/]/).pop() ?? "";
  const dot = base.lastIndexOf(".");
  const folderName =
    ctx.root
      .replace(/[\\/]+$/, "")
      .split(/[\\/]/)
      .pop() ?? "";
  const vars: Record<string, string> = {
    workspaceFolder: ctx.root.replace(/[\\/]+$/, ""),
    workspaceRoot: ctx.root.replace(/[\\/]+$/, ""),
    workspaceFolderBasename: folderName,
    file: fileAbs,
    relativeFile: ctx.file.split("/").join(sep),
    relativeFileDirname: ctx.file.includes("/")
      ? ctx.file.slice(0, ctx.file.lastIndexOf("/")).split("/").join(sep)
      : "",
    fileBasename: base,
    fileBasenameNoExtension: dot > 0 ? base.slice(0, dot) : base,
    fileExtname: dot > 0 ? base.slice(dot) : "",
    fileDirname: fileAbs ? fileAbs.slice(0, fileAbs.length - base.length - 1) : "",
    fileWorkspaceFolder: ctx.root.replace(/[\\/]+$/, ""),
    cwd: ctx.root.replace(/[\\/]+$/, ""),
    lineNumber: String(ctx.line),
    selectedText: ctx.selectedText,
    pathSeparator: sep,
    "/": sep,
    userHome: ctx.home,
    // PHP Debug fills this itself with the port it listens on (launch with "port": 0).
    port: "${port}",
  };
  const fill = (s: string): string =>
    s.replace(/\$\{([^}]+)\}/g, (all, name: string) => {
      if (name.startsWith("env:")) return ctx.env[name.slice(4)] ?? "";
      if (name in vars) return vars[name];
      unknown.add(all);
      return all;
    });
  const walk = (v: unknown): unknown => {
    if (typeof v === "string") return fill(v);
    if (Array.isArray(v)) return v.map(walk);
    if (v && typeof v === "object") return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, walk(x)]));
    return v;
  };
  return { config: walk(config) as LaunchConfig, unknown: [...unknown] };
}

const NODE_FILE = /\.(c|m)?(j|t)s$/i;
const PHP_FILE = /\.(php|phtml|inc)$/i;

const PHP_RUN: LaunchConfig = {
  name: "PHP: run the current file",
  type: "php",
  request: "launch",
  program: "${file}",
  cwd: "${fileDirname}",
  port: 0,
};
const PHP_LISTEN: LaunchConfig = {
  name: "PHP: listen for Xdebug (port 9003)",
  type: "php",
  request: "launch",
  port: 9003,
};
const NODE_RUN: LaunchConfig = {
  name: "Node: run the current file",
  type: "node",
  request: "launch",
  program: "${file}",
  cwd: "${workspaceFolder}",
};
const NODE_ATTACH: LaunchConfig = { name: "Node: attach (port 9229)", type: "node", request: "attach", port: 9229 };

/** The configurations offered when launch.json has none: run the shown file, then listen or attach. */
export function defaultConfigs(file: string): LaunchConfig[] {
  if (PHP_FILE.test(file)) return [PHP_RUN, PHP_LISTEN, NODE_ATTACH];
  if (NODE_FILE.test(file)) return [NODE_RUN, NODE_ATTACH, PHP_LISTEN];
  return [NODE_ATTACH, PHP_LISTEN];
}

/** A new launch.json with Qanuvi's defaults for the shown file, the same form VS Code writes. */
export function launchTemplate(file = ""): string {
  const configurations = PHP_FILE.test(file)
    ? [
        { name: "Listen for Xdebug", type: "php", request: "launch", port: 9003 },
        {
          name: "Launch currently open script",
          type: "php",
          request: "launch",
          program: "${file}",
          cwd: "${fileDirname}",
          port: 0,
        },
      ]
    : [
        {
          type: "node",
          request: "launch",
          name: "Launch the current file",
          program: "${file}",
          cwd: "${workspaceFolder}",
          skipFiles: ["<node_internals>/**"],
        },
      ];
  return `${JSON.stringify({ version: "0.2.0", configurations }, null, 2)}\n`;
}

/** The Xdebug settings a PHP debug run needs; the configuration's own runtimeArgs win over them. */
const XDEBUG_ARGS: [string, string][] = [
  ["xdebug.mode", "debug"],
  ["xdebug.start_with_request", "yes"],
  ["xdebug.client_host", "127.0.0.1"],
  ["xdebug.client_port", "${port}"],
];

/**
 * A configuration in the form the debugger wants. js-debug names its Node type "pwa-node". A PHP run gets the PHP
 * Qanuvi found, the chosen Xdebug file (loaded for this run only, php.ini untouched) and the Xdebug settings it needs.
 */
export function forAdapter(adapter: string, config: LaunchConfig, php?: PhpInfo | null): LaunchConfig {
  if (adapter === "node" && config.type === "node") return { ...config, type: "pwa-node" };
  if (adapter === "php" && php && (config.program || config.runtimeArgs)) {
    const own = Array.isArray(config.runtimeArgs) ? (config.runtimeArgs as unknown[]).map(String) : [];
    const has = (key: string) => own.some((a) => a.replace(/^-d\s*/, "").startsWith(`${key}=`));
    const extra: string[] = [];
    if (php.via === "file" && !has("zend_extension")) extra.push(`-dzend_extension=${php.xdebugFile ?? ""}`);
    for (const [k, v] of XDEBUG_ARGS) if (!has(k)) extra.push(`-d${k}=${v}`);
    return {
      ...config,
      runtimeExecutable: config.runtimeExecutable ?? (php.php || undefined),
      runtimeArgs: [...extra, ...own],
    };
  }
  return config;
}

/** Why Qanuvi cannot run a configuration yet, or "" when it can. */
export function unsupported(config: LaunchConfig): string {
  if (!adapterFor(config.type)) {
    return `Qanuvi has no debugger for "${config.type}" yet (Node.js and PHP with Xdebug work now).`;
  }
  if (config.type === "node-terminal") return '"node-terminal" runs in VS Code\'s terminal; use "node" here.';
  return "";
}

/** Breakpoints of a file as the setBreakpoints request wants them. */
export function sourceBreakpoints(list: Breakpoint[]): { line: number; condition?: string; logMessage?: string }[] {
  return list
    .filter((b) => b.enabled)
    .map((b) => ({
      line: b.line,
      ...(b.condition ? { condition: b.condition } : {}),
      ...(b.logMessage ? { logMessage: b.logMessage } : {}),
    }));
}

/** Adds a breakpoint at a line, or takes the one there away. */
export function toggleLine(list: Breakpoint[], line: number): Breakpoint[] {
  return list.some((b) => b.line === line)
    ? list.filter((b) => b.line !== line)
    : [...list, { line, enabled: true }].sort((a, b) => a.line - b.line);
}

/** An error response's text: the adapter's formatted error first, then its message. */
export function errorOf(response: {
  message?: string;
  body?: { error?: { format?: string; variables?: Record<string, string> } };
}): string {
  const e = response.body?.error;
  if (e?.format) {
    return e.format.replace(/\{(\w+)\}/g, (all, k: string) => e.variables?.[k] ?? all);
  }
  return response.message || "The debugger answered with an error.";
}
