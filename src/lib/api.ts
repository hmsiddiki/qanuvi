/** Typed calls into the Rust core (src-tauri/src/lib.rs). */
import { Channel } from "@tauri-apps/api/core";
// Calls for a server's projects go to that server (Milestone 16); the rest to this PC.
import { routed as invoke } from "./remote";

export interface ProjectSettings {
  run_command: string;
  test_command: string;
  preview_url: string;
}

export interface Project {
  id: string;
  name: string;
  path: string;
  kind: string;
  tags: string[];
  settings: ProjectSettings;
  added_at: number;
  last_opened_at: number;
  /** The server (master without a window) the project is on; absent for this PC. Set by the window only. */
  master?: string;
}

export interface Entry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  /** Left out by .gitignore (shown greyed). */
  ignored: boolean;
}

export interface FileContent {
  path: string;
  text: string;
  hash: string;
  size: number;
  binary: boolean;
  too_large: boolean;
}

export interface SearchMatch {
  path: string;
  line: number;
  column: number;
  text: string;
  /** Where the matches are in `text` (characters, [start, end)). */
  spans: [number, number][];
  /** How many matches the line has. */
  count: number;
}

/** How to search (15k). */
export interface SearchOptions {
  regex: boolean;
  caseSensitive: boolean;
  wholeWord: boolean;
  /** Comma-separated globs, for example "src/**, *.php"; empty: every file. */
  include: string;
  exclude: string;
}

export interface SearchResult {
  matches: SearchMatch[];
  files_searched: number;
  truncated: boolean;
}

export interface Shell {
  id: string;
  label: string;
  program: string;
  args: string[];
}

export type TermEvent = { kind: "data"; data: string } | { kind: "exit"; code: number | null };

/** What a language server sends (15i): whole protocol messages, lines of its log, its end. */
export type LspEvent =
  { kind: "message"; text: string } | { kind: "log"; text: string } | { kind: "exit"; code: number | null };

/** A language server Qanuvi knows, and whether it is on this computer. */
export interface LspStatus {
  id: string;
  label: string;
  serves: string;
  /** "qanuvi" (installed by Qanuvi), "path" (found on this computer), "" (not installed). */
  found: "qanuvi" | "path" | "";
  /** What installing does; empty when it cannot be installed here. */
  install: string;
  /** Why it cannot be installed here. */
  missing: string;
}

export interface GitChange {
  path: string;
  status: string;
  staged: boolean;
  untracked: boolean;
  deleted: boolean;
}

export interface GitStatus {
  is_repo: boolean;
  branch: string;
  changes: GitChange[];
  upstream: string;
  ahead: number;
  behind: number;
}

/** A file in the last commit and in the staging area (null where it is not there). */
export interface GitVersions {
  head: string | null;
  index: string | null;
}

export interface GitBranch {
  name: string;
  current: boolean;
  remote: boolean;
}

export interface GitStash {
  index: number;
  message: string;
}

export interface GitCommit {
  sha: string;
  short: string;
  author: string;
  date: string;
  subject: string;
}

export interface GitBlame {
  sha: string;
  author: string;
  time: number;
  summary: string;
  uncommitted: boolean;
}

/** A standing approval: "edits" (all file changes) or a tool name; secondsLeft null while the session runs. */
export interface Grant {
  group: string;
  secondsLeft: number | null;
}

export interface AgentInfo {
  available: boolean;
  path: string;
  version: string;
}

export interface AgentConfigFile {
  id: string;
  label: string;
  scope: "user" | "project";
  display: string;
  exists: boolean;
}

/** An agent session that changed a file. */
export interface FileTouch {
  sessionId: string;
  title: string;
  agent: string;
  model: string;
  updatedAt: number;
  edits: number;
}

/** A file or web page attached to a message: an image the agent sees, or text it reads. */
export interface Attachment {
  name: string;
  kind: "image" | "text" | "page";
  mediaType: string;
  data: string;
}

export interface Session {
  id: string;
  project_id: string;
  title: string;
  agent: string;
  model: string;
  mode: string;
  status: "running" | "waiting" | "idle" | "stopped" | "failed" | "ended";
  agent_session_id: string | null;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cost_usd: number;
  duration_ms: number;
  created_at: number;
  updated_at: number;
}

export type AgentEvent =
  | { kind: "line"; line: string }
  | { kind: "session"; session: Session }
  | { kind: "error"; text: string }
  | { kind: "exit"; code: number | null };

/** Errors from the core arrive as plain strings; make sure callers always get readable text. */
export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}

export interface AgentView {
  url: string;
  title: string;
  updated: number;
  shot: boolean;
  shot_updated: number | null;
}

export interface InspectorShot {
  image: string;
  url: string;
  title: string;
  scroll: number;
  width: number;
  height: number;
}

export interface PickedElement {
  selector: string;
  what: string;
  text: string;
  html: string;
  url: string;
  title: string;
  /** Ready-made text to hand the agent. */
  note: string;
}

/** An entry of the user's memory (Milestone 17). */
export interface MemoryEntry {
  id: string;
  kind: "rule" | "style" | "fact" | "decision" | "solution" | "example";
  /** "" for every project, else a project id. */
  scope: string;
  title: string;
  body: string;
  tags: string;
  status: "active" | "suggested";
  source: "user" | "agent";
  created_at: number;
  updated_at: number;
  uses: number;
}

export interface MemorySlice {
  text: string;
  ids: string[];
  left_out: number;
}

export const api = {
  startupPath: () => invoke<string | null>("startup_path"),
  projects: {
    list: () => invoke<Project[]>("projects_list"),
    add: (path: string) => invoke<Project>("projects_add", { path }),
    remove: (id: string) => invoke<void>("projects_remove", { id }),
    update: (id: string, name: string, settings: ProjectSettings) =>
      invoke<Project>("projects_update", { id, name, settings }),
    touch: (id: string) => invoke<void>("projects_touch", { id }),
    redetect: (id: string) => invoke<Project>("projects_redetect", { id }),
  },
  files: {
    listDir: (projectId: string, path: string) => invoke<Entry[]>("files_list_dir", { projectId, path }),
    listAll: (projectId: string) => invoke<{ files: string[]; truncated: boolean }>("files_list_all", { projectId }),
    /** Every Markdown file, ignore rules or not (task lists can sit in any folder). */
    markdown: (projectId: string) => invoke<string[]>("files_markdown", { projectId }),
    /** Files modified since a moment (ms since 1970): what a plan step changed. */
    changedSince: (projectId: string, sinceMs: number) =>
      invoke<string[]>("files_changed_since", { projectId, sinceMs: Math.floor(sinceMs) }),
    /** Adds to a plan step's log (under .qanuvi/plan-logs). */
    logAppend: (projectId: string, path: string, text: string) =>
      invoke<void>("files_log_append", { projectId, path, text }),
    read: (projectId: string, path: string) => invoke<FileContent>("files_read", { projectId, path }),
    write: (projectId: string, path: string, text: string, expectedHash: string | null) =>
      invoke<string>("files_write", { projectId, path, text, expectedHash }),
    /** A new empty file or folder (never replaces one that is there). */
    create: (projectId: string, path: string, folder: boolean) =>
      invoke<void>("files_create", { projectId, path, folder }),
    /** Renames or moves a file or folder inside the project (never replaces). */
    rename: (projectId: string, from: string, to: string) => invoke<void>("files_rename", { projectId, from, to }),
    /** Moves a file or folder to the Recycle Bin. */
    trash: (projectId: string, path: string) => invoke<void>("files_trash", { projectId, path }),
    search: (projectId: string, query: string, regex: boolean, caseSensitive: boolean) =>
      invoke<SearchResult>("files_search", { projectId, query, regex, caseSensitive }),
    /** Search with every option: whole words, files to include and leave out. */
    find: (projectId: string, query: string, options: SearchOptions) =>
      invoke<SearchResult>("files_search", {
        projectId,
        query,
        regex: options.regex,
        caseSensitive: options.caseSensitive,
        options,
      }),
    /** Replaces the matches in the project's files (only in `paths` when given). */
    replace: (projectId: string, query: string, options: SearchOptions, replacement: string, paths?: string[]) =>
      invoke<{ files: number; replacements: number }>("files_replace", {
        projectId,
        query,
        options,
        replacement,
        paths: paths ?? null,
      }),
  },
  term: {
    shells: () => invoke<Shell[]>("term_shells"),
    spawn: (
      projectId: string,
      shell: string,
      cols: number,
      rows: number,
      onEvent: (e: TermEvent) => void,
      dir: string | null = null,
    ) => {
      const channel = new Channel<TermEvent>();
      channel.onmessage = onEvent;
      return invoke<string>("term_spawn", { projectId, shell, cols, rows, dir, onEvent: channel });
    },
    write: (id: string, data: string) => invoke<void>("term_write", { id, data }),
    resize: (id: string, cols: number, rows: number) => invoke<void>("term_resize", { id, cols, rows }),
    kill: (id: string) => invoke<void>("term_kill", { id }),
    /** Stops this window's terminals (before the interface reloads). */
    killWindow: () => invoke<void>("term_kill_window"),
  },
  lsp: {
    statuses: () => invoke<LspStatus[]>("lsp_statuses"),
    install: (id: string) => invoke<LspStatus>("lsp_install", { id }),
    start: (projectId: string, id: string, onEvent: (e: LspEvent) => void) => {
      const channel = new Channel<LspEvent>();
      channel.onmessage = onEvent;
      return invoke<string>("lsp_start", { projectId, id, onEvent: channel });
    },
    send: (key: string, text: string) => invoke<void>("lsp_send", { key, text }),
    stop: (key: string) => invoke<void>("lsp_stop", { key }),
    /** Stops this window's servers (before the interface reloads). */
    stopWindow: () => invoke<void>("lsp_stop_window"),
  },
  git: {
    status: (projectId: string) => invoke<GitStatus>("git_status", { projectId }),
    diff: (projectId: string, path: string) => invoke<string>("git_diff", { projectId, path }),
    stage: (projectId: string, path: string) => invoke<void>("git_stage", { projectId, path }),
    revert: (projectId: string, path: string) => invoke<void>("git_revert", { projectId, path }),
    versions: (projectId: string, path: string) => invoke<GitVersions>("git_versions", { projectId, path }),
    setIndex: (projectId: string, path: string, text: string) =>
      invoke<void>("git_set_index", { projectId, path, text }),
    unstage: (projectId: string, path: string) => invoke<void>("git_unstage", { projectId, path }),
    stageAll: (projectId: string) => invoke<void>("git_stage_all", { projectId }),
    commit: (projectId: string, message: string, amend: boolean, all: boolean) =>
      invoke<string>("git_commit", { projectId, message, amend, all }),
    sync: (projectId: string, action: "fetch" | "pull" | "push") => invoke<string>("git_sync", { projectId, action }),
    branches: (projectId: string) => invoke<GitBranch[]>("git_branches", { projectId }),
    switch: (projectId: string, name: string, create: boolean) =>
      invoke<void>("git_switch", { projectId, name, create }),
    stashes: (projectId: string) => invoke<GitStash[]>("git_stashes", { projectId }),
    stash: (projectId: string, action: "push" | "pop" | "apply" | "drop", index = 0, message = "") =>
      invoke<void>("git_stash", { projectId, action, index, message }),
    log: (projectId: string, path: string | null, max = 50) => invoke<GitCommit[]>("git_log", { projectId, path, max }),
    show: (projectId: string, sha: string, path: string | null) => invoke<string>("git_show", { projectId, sha, path }),
    blame: (projectId: string, path: string, line: number) => invoke<GitBlame>("git_blame", { projectId, path, line }),
  },
  /** Files and web pages for the next message (the composer's + menu). */
  attachments: {
    file: (path: string) => invoke<Attachment>("attachment_file", { path }),
    page: (url: string) => invoke<Attachment>("attachment_page", { url }),
  },
  /** Qanuvi's remote knowledge (24c): the signed file's text, its signature checked. */
  knowledge: {
    fetch: (url = "") => invoke<string>("knowledge_fetch", { url }),
  },
  /** Qanuvi core's own models (24b). */
  core: {
    /** A short decision from the free models, asked one after another: [the model that answered, its text]. */
    decide: (setup: string, system: string, prompt: string) =>
      invoke<[string, string]>("core_decide", { setup, system, prompt }),
  },
  agent: {
    info: () => invoke<AgentInfo>("agent_info"),
    sessions: (projectId: string) => invoke<Session[]>("agent_sessions", { projectId }),
    events: (sessionId: string) => invoke<string[]>("agent_session_events", { sessionId }),
    /** When each event line arrived (seconds; 0 for old lines), in the order of `events`. */
    times: (sessionId: string) => invoke<number[]>("agent_session_times", { sessionId }),
    /** The sessions that changed a file, newest first (the project panel's Timeline). */
    fileHistory: (projectId: string, path: string) => invoke<FileTouch[]>("agent_file_history", { projectId, path }),
    start: (
      projectId: string,
      sessionId: string | null,
      prompt: string,
      model: string,
      mode: string,
      baseUrl: string,
      effort: string,
      onEvent: (e: AgentEvent) => void,
      lean = false,
      indexTools = false,
      browserTools = false,
      context: string | null = null,
      agent = "claude-code",
      setup: unknown = null,
      files: Attachment[] = [],
    ) => {
      const channel = new Channel<AgentEvent>();
      channel.onmessage = onEvent;
      return invoke<Session>("agent_start", {
        agent,
        setup,
        projectId,
        sessionId,
        prompt,
        model,
        mode,
        baseUrl: baseUrl.trim() || null,
        effort: effort || null,
        lean,
        indexTools,
        browserTools,
        context,
        files: files.length ? files : null,
        onEvent: channel,
      });
    },
    /** remember: "session" or "minutes" answers later requests of the same kind without asking (risky commands still ask). */
    respond: (sessionId: string, requestId: string, allow: boolean, remember = "") =>
      invoke<void>("agent_respond", { sessionId, requestId, allow, remember }),
    grants: (sessionId: string) => invoke<Grant[]>("agent_grants", { sessionId }),
    forgetGrants: (sessionId: string) => invoke<void>("agent_forget_grants", { sessionId }),
    stop: (sessionId: string) => invoke<void>("agent_stop", { sessionId }),
    /** Ends the turn and its command, keeps the session: a waiting message is read next (24r). */
    interrupt: (sessionId: string) => invoke<void>("agent_interrupt", { sessionId }),
    /** A project's secrets: only their names come back. */
    secretNames: (projectId: string) => invoke<string[]>("secrets_names", { projectId }),
    setSecret: (projectId: string, name: string, value: string) =>
      invoke<void>("secrets_set", { projectId, name, value }),
    removeSecret: (projectId: string, name: string) => invoke<void>("secrets_remove", { projectId, name }),
    /** Private details given during a plan's run, kept for that plan in the credential store. */
    planPrivate: (projectId: string, file: string) => invoke<string[]>("plan_private", { projectId, file }),
    planPrivateAdd: (projectId: string, file: string, text: string) =>
      invoke<void>("plan_private_add", { projectId, file, text }),
    planPrivateClear: (projectId: string, file: string) => invoke<void>("plan_private_clear", { projectId, file }),
    /** Changes a running session's model or mode now. */
    configure: (sessionId: string, change: { model?: string; mode?: string }) =>
      invoke<void>("agent_configure", { sessionId, model: change.model ?? null, mode: change.mode ?? null }),
    rename: (sessionId: string, title: string) => invoke<void>("agent_rename", { sessionId, title }),
    remove: (sessionId: string) => invoke<void>("agent_delete", { sessionId }),
  },
  browser: {
    /** The page the agent's browser tools are on in a session (null before they open one). */
    agentView: (sessionId: string) => invoke<AgentView | null>("browser_agent_view", { sessionId }),
    agentShot: (sessionId: string) => invoke<string>("browser_agent_shot", { sessionId }),
    show: (url: string, width: number, height: number) =>
      invoke<InspectorShot>("inspector_show", { url, width, height }),
    scroll: (dy: number) => invoke<InspectorShot>("inspector_scroll", { dy }),
    pick: (x: number, y: number) => invoke<PickedElement>("inspector_pick", { x, y }),
    pageText: (url: string) => invoke<string>("inspector_page_text", { url }),
    close: () => invoke<void>("inspector_close"),
  },
  memory: {
    /** Entries for a project (and every project), or all with null; suggested ones too with `suggested`. */
    list: (project: string | null, suggested = false) => invoke<MemoryEntry[]>("memory_list", { project, suggested }),
    save: (entry: Partial<MemoryEntry> & Pick<MemoryEntry, "kind" | "title">) =>
      invoke<MemoryEntry>("memory_save", { entry }),
    remove: (id: string) => invoke<void>("memory_delete", { id }),
    /** With `count`, the entries are counted as given to an agent (not for a preview). */
    slice: (project: string | null, query: string, budget: number, count = false) =>
      invoke<MemorySlice>("memory_slice", { project, query, budget, count }),
    search: (project: string | null, query: string) => invoke<MemoryEntry[]>("memory_search", { project, query }),
  },
  checks: {
    run: (projectId: string, command: string, timeoutSecs: number) =>
      invoke<{ ok: boolean; exit_code: number | null; timed_out: boolean; duration_ms: number; output: string }>(
        "checks_run",
        { projectId, command, timeoutSecs },
      ),
  },
  index: {
    find: (projectId: string, query: string) =>
      invoke<{ path: string; name: string; kind: string; line: number; container: string | null }[]>("index_find", {
        projectId,
        query,
      }),
    outline: (projectId: string, path: string) =>
      invoke<{ name: string; kind: string; line: number; container: string | null }[]>("index_outline", {
        projectId,
        path,
      }),
  },
  usage: {
    since: (since: number) =>
      invoke<{ cost_usd: number; sessions: number; input_tokens: number; output_tokens: number }>("agent_usage_since", {
        since,
      }),
  },
  runs: {
    listAll: (limit: number) =>
      invoke<{ id: string; file: string; status: string; data: string; created_at: number; updated_at: number }[]>(
        "runs_list_all",
        { limit },
      ),
    save: (projectId: string, id: string, file: string, status: string, data: string) =>
      invoke<void>("runs_save", { projectId, id, file, status, data }),
    list: (projectId: string) =>
      invoke<{ id: string; file: string; status: string; data: string; created_at: number; updated_at: number }[]>(
        "runs_list",
        { projectId },
      ),
  },
  agentConfig: {
    list: (provider: string, projectId: string | null) =>
      invoke<AgentConfigFile[]>("agent_config_list", { provider, projectId }),
    read: (provider: string, id: string, projectId: string | null) =>
      invoke<FileContent>("agent_config_read", { provider, id, projectId }),
    write: (provider: string, id: string, projectId: string | null, text: string, expectedHash: string | null) =>
      invoke<string>("agent_config_write", { provider, id, projectId, text, expectedHash }),
  },
};
