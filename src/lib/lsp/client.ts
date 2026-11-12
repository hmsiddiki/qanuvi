/**
 * One running language server for one project (15i), spoken to with JSON-RPC. The Rust side only frames the
 * messages; requests, answers, the server's own requests and the open documents are handled here.
 */
import { api, type LspEvent } from "$lib/api";
import { fileUri, languageId, type ServerId } from "./protocol";

type Handler = (params: unknown) => void;

interface Pending {
  resolve: (v: unknown) => void;
  reject: (e: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

/** What the server says it can do (only the parts Qanuvi uses). */
export interface Capabilities {
  hoverProvider?: unknown;
  completionProvider?: { triggerCharacters?: string[]; resolveProvider?: boolean };
  definitionProvider?: unknown;
  referencesProvider?: unknown;
  renameProvider?: unknown;
  textDocumentSync?: number | { change?: number; save?: unknown };
}

const TIMEOUT_MS = 15_000;

export class LspClient {
  key = "";
  capabilities: Capabilities = {};
  ready: Promise<void>;
  /** The last lines of the server's error output, for its log. */
  log: string[] = [];
  ended = false;
  #next = 1;
  #pending = new Map<number, Pending>();
  #handlers = new Map<string, Handler[]>();
  #versions = new Map<string, number>();
  #onExit: (why: string) => void;

  constructor(
    readonly projectId: string,
    readonly server: ServerId,
    readonly root: string,
    onExit: (why: string) => void,
  ) {
    this.#onExit = onExit;
    this.ready = this.#start();
  }

  async #start(): Promise<void> {
    this.key = await api.lsp.start(this.projectId, this.server, (e) => this.#event(e));
    const rootUri = fileUri(this.root);
    const name = this.root.replace(/\\/g, "/").split("/").pop() ?? "project";
    const result = (await this.request(
      "initialize",
      {
        processId: null,
        clientInfo: { name: "Qanuvi" },
        rootUri,
        rootPath: this.root,
        workspaceFolders: [{ uri: rootUri, name }],
        capabilities: {
          general: { positionEncodings: ["utf-16"] },
          textDocument: {
            synchronization: { didSave: true, dynamicRegistration: false },
            publishDiagnostics: { relatedInformation: false, versionSupport: true },
            hover: { contentFormat: ["markdown", "plaintext"] },
            completion: {
              completionItem: {
                snippetSupport: false,
                documentationFormat: ["markdown", "plaintext"],
                insertReplaceSupport: false,
              },
              contextSupport: true,
            },
            definition: { linkSupport: true },
            references: {},
            rename: { prepareSupport: false },
          },
          workspace: { workspaceFolders: true, configuration: true, applyEdit: false },
          window: { workDoneProgress: false },
        },
        // Intelephense keeps its index in a folder of its own choosing; nothing else to set here.
        initializationOptions: {},
      },
      60_000,
    )) as { capabilities?: Capabilities };
    this.capabilities = result?.capabilities ?? {};
    this.notify("initialized", {});
  }

  #event(e: LspEvent) {
    if (e.kind === "log") {
      this.log.push(e.text);
      if (this.log.length > 200) this.log.splice(0, this.log.length - 200);
      return;
    }
    if (e.kind === "exit") {
      this.ended = true;
      for (const p of this.#pending.values()) {
        clearTimeout(p.timer);
        p.reject(new Error("The language server stopped."));
      }
      this.#pending.clear();
      this.#onExit(this.log.slice(-3).join("\n"));
      return;
    }
    let msg: { id?: number | string; method?: string; params?: unknown; result?: unknown; error?: { message: string } };
    try {
      msg = JSON.parse(e.text);
    } catch {
      return;
    }
    if (msg.method && msg.id !== undefined) {
      this.#answer(msg.id, msg.method, msg.params);
    } else if (msg.method) {
      for (const h of this.#handlers.get(msg.method) ?? []) h(msg.params);
    } else if (typeof msg.id === "number") {
      const p = this.#pending.get(msg.id);
      if (!p) return;
      this.#pending.delete(msg.id);
      clearTimeout(p.timer);
      if (msg.error) p.reject(new Error(msg.error.message));
      else p.resolve(msg.result);
    }
  }

  /** Requests the server makes of the editor. Unknown ones get an empty answer, so the server goes on. */
  #answer(id: number | string, method: string, params: unknown) {
    let result: unknown = null;
    if (method === "workspace/configuration") {
      const items = (params as { items?: unknown[] })?.items ?? [];
      result = items.map(() => null);
    } else if (method === "workspace/workspaceFolders") {
      result = [{ uri: fileUri(this.root), name: "project" }];
    }
    this.#write({ jsonrpc: "2.0", id, result });
  }

  #write(msg: unknown) {
    if (this.ended || !this.key) return;
    api.lsp.send(this.key, JSON.stringify(msg)).catch(() => {});
  }

  on(method: string, handler: Handler): void {
    this.#handlers.set(method, [...(this.#handlers.get(method) ?? []), handler]);
  }

  request<T = unknown>(method: string, params: unknown, timeout = TIMEOUT_MS): Promise<T> {
    if (this.ended) return Promise.reject(new Error("The language server stopped."));
    const id = this.#next++;
    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.#pending.delete(id);
        reject(new Error(`The language server did not answer ${method} in time.`));
      }, timeout);
      this.#pending.set(id, { resolve: resolve as (v: unknown) => void, reject, timer });
      this.#write({ jsonrpc: "2.0", id, method, params });
    });
  }

  notify(method: string, params: unknown): void {
    this.#write({ jsonrpc: "2.0", method, params });
  }

  // Documents ---------------------------------------------------------------------------------------------------

  isOpen(uri: string): boolean {
    return this.#versions.has(uri);
  }

  open(uri: string, path: string, text: string): void {
    if (this.#versions.has(uri)) return this.change(uri, text);
    this.#versions.set(uri, 1);
    this.notify("textDocument/didOpen", { textDocument: { uri, languageId: languageId(path), version: 1, text } });
  }

  /** The whole text each time: simple, and fine for source files (servers handle it fast). */
  change(uri: string, text: string): void {
    const v = (this.#versions.get(uri) ?? 0) + 1;
    this.#versions.set(uri, v);
    this.notify("textDocument/didChange", { textDocument: { uri, version: v }, contentChanges: [{ text }] });
  }

  saved(uri: string, text: string): void {
    if (!this.#versions.has(uri)) return;
    this.notify("textDocument/didSave", { textDocument: { uri }, text });
  }

  close(uri: string): void {
    if (!this.#versions.delete(uri)) return;
    this.notify("textDocument/didClose", { textDocument: { uri } });
  }

  get openCount(): number {
    return this.#versions.size;
  }

  /** Asks the server to end, then stops it whatever it answers. */
  async stop(): Promise<void> {
    if (this.ended) return;
    try {
      await this.ready;
      await this.request("shutdown", null, 3000);
      this.notify("exit", null);
    } catch {
      // A server that does not answer is stopped below anyway.
    }
    if (this.key) await api.lsp.stop(this.key).catch(() => {});
    this.ended = true;
  }
}
