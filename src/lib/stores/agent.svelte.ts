import { api, errorText, type AgentEvent, type AgentInfo, type Attachment, type Grant, type Session } from "$lib/api";
import { untrack } from "svelte";
import { reduce, reduceAll, type Item } from "$lib/agent/events";
import { HOLD_MS, withdrawnNote } from "$lib/agent/waiting";

const nowSeconds = () => Math.floor(Date.now() / 1000);
import { settings } from "./settings.svelte";
import { workspace } from "./workspace.svelte";
import { memoryContext } from "./memory.svelte";
import { modelLabel, paidAgent, providerById, type AgentProvider } from "$lib/agents/providers";
import { chatModelPrompt, deciderSeat, overruleNote, parseChatModel, strongerModel } from "$lib/core/decider";
import { itemsToMarkdown } from "$lib/agent/events";
import { freeDecide, ownSetup, setupFor } from "./brain.svelte";
import { snapshotChanges } from "./changes";
import { branchPreamble } from "$lib/agent/sessionTools";
import { contextSize } from "$lib/plan/guidance";
import {
  chainOf,
  chatHandoverDue,
  chatHandoverRequest,
  chatResumePrompt,
  chatsOnly,
  forgetNote,
  keepNote,
  keptNote,
  lastRequest,
  noteAfter,
  reopenFresh,
} from "$lib/agent/handover";

/** An agent that does not answer the handover request within this time is stopped; its last words stand in. */
const HANDOVER_WAIT_MS = 180_000;

/** Agent sessions of the current project and the conversation of the selected one. */
class AgentStore {
  info = $state<AgentInfo | null>(null);
  sessions = $state<Session[]>([]);
  selectedId = $state<string | null>(null);
  /** Files and pages added for the next message (the composer's + menu). */
  attachments = $state<Attachment[]>([]);
  /** Standing approvals of the selected session. */
  grants = $state<Grant[]>([]);
  items = $state<Item[]>([]);
  errors = $state<string[]>([]);
  providerId = $state(settings.value.agent);
  model = $state(settings.agent(settings.value.agent).model);
  mode = $state(settings.agent(settings.value.agent).mode);
  effort = $state(settings.agent(settings.value.agent).effort);
  busy = $state(false);
  /** Text in the message box (other views can fill it, for example "write a task list"). */
  draft = $state("");
  input: HTMLTextAreaElement | undefined = $state();
  error = $state("");
  /** Goes up when an agent turn ends, so views that show files or git status can refresh. */
  turnsEnded = $state(0);
  #projectId: string | null = null;
  #seq = 0;
  /** Messages sent while the agent works, kept back for a few seconds (24r): by their item id. */
  #held = new Map<string, { projectId: string; text: string; timer: ReturnType<typeof setTimeout> }>();
  /** Per session: messages withdrawn after the agent had them; its next message tells it to leave them. */
  #withdrawn = new Map<string, string[]>();

  get provider(): AgentProvider {
    return providerById(this.providerId);
  }

  /** Next permission mode, for Shift+Tab in the message box. */
  cycleMode(): void {
    const modes = this.provider.modes;
    const i = modes.findIndex((m) => m.id === this.mode);
    this.mode = modes[(i + 1) % modes.length].id;
  }

  get selected(): Session | null {
    return this.sessions.find((s) => s.id === this.selectedId) ?? null;
  }

  /** The chats to list (24s): sessions another one goes on from show through that one. */
  get chats(): Session[] {
    return chatsOnly(this.sessions);
  }

  /** The selected session's context size now: what its last model call read (24s). */
  context = $state(0);
  /** The bar "This chat is heavy, hand over now?" shows (Chat handover: Ask me first). */
  offer = $state(false);
  /** A handover in progress: the session handing over, its size, where the request is, messages held for the new one. */
  handover = $state<{
    fromId: string;
    was: number;
    requestAt: number;
    queued: string[];
    /** Reopening a starred chat (24s part 3): only the note is wanted; a new session starts when a message is sent. */
    noteOnly?: boolean;
  } | null>(null);
  /**
   * A starred chat reopened (24s part 3): the next message starts a fresh session from a note (edited by the owner
   * when they like) instead of re-reading the old session. `note` is null until it is written; `lines` stamps it.
   */
  reopened = $state<{ id: string; was: number; note: string | null; lines: number } | null>(null);
  /** Per session: the context size at which the owner said "Not now". */
  #notNow = new Map<string, number>();
  /**
   * A session that just took a chat over: its first measured size (the agent's own start plus the note) is where it
   * counts from, so a low limit does not offer a handover again at once, over and over.
   */
  #fromHandover = false;
  #handoverTimer: ReturnType<typeof setTimeout> | null = null;

  /** Whether to offer or start a handover now (after a model call's size is known, or a turn's end). */
  #checkHandover(): void {
    const s = this.selected;
    if (!s || this.handover || this.planSessionId === s.id || !this.#projectId) return;
    if (this.#fromHandover && this.context > 0) {
      this.#fromHandover = false;
      this.#notNow.set(s.id, this.context);
    }
    const setting = settings.value.chatHandover;
    if (!chatHandoverDue(this.context, setting, !!s.handover_off, this.#notNow.get(s.id) ?? 0)) {
      this.offer = false;
      return;
    }
    if (setting.when === "auto") void this.handOver();
    else this.offer = true;
  }

  /** "Not now": the bar goes until the context has grown a good deal more. */
  notNow(): void {
    if (this.selectedId) this.#notNow.set(this.selectedId, this.context);
    this.offer = false;
  }

  /** "Never in this chat": this chat no longer hands over by itself. */
  async handoverOffForChat(off = true): Promise<void> {
    const s = this.selected;
    if (!s) return;
    try {
      await api.agent.handoverOff(s.id, off);
      s.handover_off = off;
      if (off) this.offer = false;
    } catch (e) {
      this.error = errorText(e);
    }
  }

  /**
   * Hands the chat over (24s): the agent finishes the edit in hand and answers with a note; when its turn ends, a new
   * session starts from the note and the last request and goes on in the same chat.
   */
  async handOver(noteOnly = false): Promise<void> {
    const s = this.selected;
    const projectId = this.#projectId;
    if (!s || !projectId || this.handover) return;
    this.offer = false;
    const was = this.context;
    this.handover = { fromId: s.id, was, requestAt: this.items.length, queued: [], noteOnly };
    try {
      await this.#write(projectId, s, chatHandoverRequest(was));
      this.busy = true;
      if (this.#handoverTimer) clearTimeout(this.#handoverTimer);
      this.#handoverTimer = setTimeout(() => {
        if (this.handover?.fromId !== s.id) return;
        api.agent.stop(s.id).catch(() => {});
        void this.#finishHandover();
      }, HANDOVER_WAIT_MS);
    } catch (e) {
      this.handover = null;
      this.error = errorText(e);
    }
  }

  /** Writes Qanuvi's words into a session (a handover request): its own model and mode, nothing else added. */
  async #write(projectId: string, s: Session, text: string): Promise<void> {
    const a = settings.agent(s.agent);
    await api.agent.start(
      projectId,
      s.id,
      text,
      s.model,
      s.mode,
      a.baseUrl,
      this.effort,
      this.handler(() => s.id),
      false,
      a.indexTools,
      a.browserTools,
      null,
      s.agent,
      setupFor(s.agent),
      [],
    );
  }

  /** The old session answered with its note (or was stopped): the new session starts in the same chat. */
  async #finishHandover(): Promise<void> {
    const h = this.handover;
    const projectId = this.#projectId;
    if (!h || !projectId) return;
    if (this.#handoverTimer) clearTimeout(this.#handoverTimer);
    this.#handoverTimer = null;
    const from = this.sessions.find((x) => x.id === h.fromId);
    // The chat moved on meanwhile (another chat was opened): nothing is started behind the owner's back.
    if (this.selectedId !== h.fromId || !from) {
      this.handover = null;
      return;
    }
    const note = noteAfter(this.items, h.requestAt, () =>
      itemsToMarkdown("The end of the earlier session", this.items.slice(0, h.requestAt)).slice(-6000),
    );
    // Reopening a starred chat, nothing sent yet: the note is kept and shown for reading and editing (24s part 3).
    if (h.noteOnly && !h.queued.length) {
      const lines = (await api.agent.events(from.id).catch(() => [] as string[])).length;
      keepNote(localStorage, from.id, lines, note);
      this.handover = null;
      this.busy = false;
      if (this.selectedId === from.id) this.reopened = { id: from.id, was: h.was, note, lines };
      return;
    }
    // A reopened chat's old last request is done or stale; the owner's new message says what comes next.
    await this.#goOnFresh(
      from,
      note,
      h.noteOnly ? "" : lastRequest(this.items, h.requestAt),
      h.was,
      h.queued,
      h.requestAt,
    );
  }

  /** Edits the note a reopened starred chat goes on from (kept with the same stamp). */
  editReopenNote(note: string): void {
    const r = this.reopened;
    if (!r || r.note === null) return;
    r.note = note;
    keepNote(localStorage, r.id, r.lines, note);
  }

  /** "Resume as it was": the reopened chat goes on in its old session, re-reading all of it. */
  resumeAsItWas(): void {
    this.reopened = null;
    this.#checkHandover();
  }

  /** The chat goes on in a new session that starts from `note`; `queued` messages join its first turn. */
  async #goOnFresh(
    from: Session,
    note: string,
    request: string,
    was: number,
    queued: string[],
    requestAt = this.items.length,
  ): Promise<void> {
    const projectId = this.#projectId;
    if (!projectId) return;
    // A plan step's chat keeps the plan's logins in its new session (at the end: not part of the shown note).
    const prompt = chatResumePrompt(note, request, was) + ((await this.privateNote?.(projectId, from.id)) ?? "");
    // The earlier part stays on screen, read only; the new session's lines follow it. Messages held for the new
    // session leave the old part: they are sent again below.
    const held = new Set(queued);
    this.items = this.items
      .filter((i, k) => k < requestAt || !(i.kind === "user" && i.waiting && held.has(i.text)))
      .map((i) => ({ ...i, older: true }));
    this.selectedId = null;
    this.providerId = from.agent;
    this.model = from.model;
    this.mode = from.mode;
    this.context = 0;
    this.#fromHandover = true;
    const ok = await this.send(projectId, prompt);
    const id = this.selectedId as string | null;
    if (!ok || !id) {
      this.#fromHandover = false;
      this.handover = null;
      this.busy = false;
      await this.select(from.id);
      this.error ||= "The new session did not start; the chat stays as it was.";
      return;
    }
    await api.agent.link(id, from.id).catch((e) => (this.error = errorText(e)));
    await this.rename(id, from.title).catch(() => {});
    const now = this.sessions.find((x) => x.id === id);
    if (now) {
      now.continues_from = from.id;
      now.handover_off = from.handover_off;
      // The star moves to the new session (agent::link does the same in the database).
      now.saved = !!(now.saved || from.saved);
    }
    from.saved = false;
    this.handover = null;
    // Messages written during the handover go to the new session, in order (they join its first turn).
    for (const text of queued) await this.send(projectId, text);
  }

  /** Adds text to the message box (after what is there), for example a page or an element from the preview. */
  attach(text: string): void {
    this.draft = this.draft.trim()
      ? `${this.draft.trimEnd()}

${text}`
      : text;
    queueMicrotask(() => this.input?.focus());
  }

  /** Puts a ready-made request in the message box for the user to read, change and send. */
  prepare(text: string): void {
    this.draft = text;
    queueMicrotask(() => this.input?.focus());
  }

  /**
   * The agent a plan run uses (24e): a paid agent that is active always sits on top, whichever agent new sessions
   * use; without one, the agent chosen for new sessions.
   */
  get runProviderId(): string {
    // Free decides, free works: runs go to Qanuvi's own agent with the free models.
    if (settings.value.team.mode === "free-only" && settings.value.brain.models.length) return "qanuvi";
    const paid = paidAgent();
    return this.info?.available && paid ? paid.id : settings.value.agent;
  }

  async loadInfo(): Promise<void> {
    try {
      this.info = await api.agent.info();
    } catch {
      this.info = { available: false, path: "", version: "" };
    }
    // No paid agent here but free models are set up: new sessions use Qanuvi's own agent.
    if (!this.info.available && settings.value.agent === "claude-code" && settings.value.brain.models.length) {
      settings.set("agent", "qanuvi");
      if (!this.selectedId) this.newSession();
    }
  }

  async loadProject(projectId: string | null): Promise<void> {
    this.#projectId = projectId;
    this.sessions = [];
    this.newSession();
    if (!projectId) return;
    try {
      const list = await api.agent.sessions(projectId);
      if (this.#projectId === projectId) this.sessions = list;
    } catch (e) {
      this.error = errorText(e);
    }
  }

  /**
   * Chat clean-up (24t): deletes chats not opened for `days` days. The chat open now and the one each project was
   * last on stay (as do starred, plan-step and running chats, kept by the app). Gives back how many were removed.
   */
  async cleanUpChats(days: number): Promise<number> {
    if (days <= 0) return 0;
    const keep = [...workspace.sessions(), ...(this.selectedId ? [this.selectedId] : [])];
    const removed = await api.agent.cleanUp(days, keep);
    const projectId = this.#projectId;
    if (removed > 0 && projectId) {
      const list = await api.agent.sessions(projectId).catch(() => null);
      if (list && this.#projectId === projectId) this.sessions = list;
    }
    return removed;
  }

  newSession(): void {
    this.#flushHeld();
    this.selectedId = null;
    this.reopened = null;
    this.items = [];
    this.errors = [];
    this.context = 0;
    this.offer = false;
    this.#fromHandover = false;
    // Read from settings, never back from this.providerId: an effect that calls this must not come to depend on
    // the session's agent (that made a loop between this and select(), 2026-11-11).
    const providerId = settings.value.agent;
    this.providerId = providerId;
    const a = settings.agent(providerId);
    this.model = a.model;
    this.mode = a.mode;
    this.effort = a.effort;
  }

  async select(id: string): Promise<void> {
    this.#flushHeld();
    this.selectedId = id;
    this.reopened = null;
    this.#fromHandover = false;
    this.loadGrants();
    this.errors = [];
    const s = this.selected;
    if (s) {
      this.providerId = s.agent;
      this.model = s.model;
      this.mode = s.mode;
    }
    this.offer = false;
    try {
      // A chat that was handed over shows its earlier sessions first, read only (24s).
      const chain = chainOf(this.sessions, id);
      const parts = await Promise.all(
        (chain.length ? chain : [{ id }]).map((c) =>
          Promise.all([api.agent.events(c.id), api.agent.times(c.id).catch(() => [] as number[])]),
        ),
      );
      if (this.selectedId === id) {
        const [lines] = parts[parts.length - 1];
        const older = parts
          .slice(0, -1)
          .flatMap(([l, t], k) => reduceAll(l, t).map((i) => ({ ...i, id: `p${k}:${i.id}`, older: true })));
        this.items = [...older, ...reduceAll(lines, parts[parts.length - 1][1])];
        this.#seq = lines.length;
        this.context = contextSize(lines);
        // A heavy starred chat goes on fresh from a note (24s part 3); otherwise the usual handover check.
        const now = this.selected;
        this.reopened =
          now && reopenFresh(now, this.context, now.id === this.planSessionId, settings.value.chatHandover.when)
            ? { id, was: this.context, note: keptNote(localStorage, id, lines.length), lines: lines.length }
            : null;
        if (!this.reopened) this.#checkHandover();
      }
    } catch (e) {
      this.error = errorText(e);
    }
  }

  /** The session of the plan step running now (set by the runner): its pickers show and change that step. */
  planSessionId = $state<string | null>(null);
  /** The runner hears picker changes on its step here (the mode for the rest of the run, the step's model). */
  onPlanChange: ((change: { model?: string; mode?: string }) => void) | null = null;
  /** A plan step's chat: the plan's private details (logins) for a new session of it, hidden in its first message. */
  privateNote: ((projectId: string, sessionId: string) => Promise<string>) | null = null;

  /** Whether the pickers now act on a running plan step. */
  get onPlanStep(): boolean {
    return !!this.planSessionId && this.planSessionId === this.selectedId;
  }

  /** The model picker: on a running plan step it changes that step now (and the plan's Model line). */
  pickModel(id: string): void {
    this.model = id;
    if (this.onPlanStep && this.planSessionId) {
      api.agent.configure(this.planSessionId, { model: id }).catch((e) => (this.error = errorText(e)));
      this.onPlanChange?.({ model: id });
    }
  }

  /** The mode picker: on a running plan step it changes it now and for the rest of the run. */
  pickMode(id: string): void {
    this.mode = id;
    if (this.onPlanStep && this.planSessionId) {
      api.agent.configure(this.planSessionId, { mode: id }).catch((e) => (this.error = errorText(e)));
      this.onPlanChange?.({ mode: id });
    }
  }

  #upsert(session: Session): void {
    const i = this.sessions.findIndex((s) => s.id === session.id);
    if (i >= 0) this.sessions[i] = session;
    else this.sessions = [session, ...this.sessions];
  }

  /** Passes a session's events to the pane (used by sends here and by plan runs). */
  handler(sessionId: () => string | null) {
    return (event: AgentEvent) => {
      const id = sessionId();
      if (event.kind === "session") {
        this.#upsert(event.session);
        // A plan step shows the model and mode the plan gave it.
        if (event.session.id === this.planSessionId && event.session.id === this.selectedId) {
          this.model = event.session.model;
          this.mode = event.session.mode;
        }
        if (["idle", "ended", "failed", "stopped"].includes(event.session.status)) {
          // The turn that answered the handover request ended: the new session takes over.
          const h = this.handover;
          if (h && event.session.id === h.fromId && this.items.slice(h.requestAt).some((i) => i.kind === "text")) {
            void this.#finishHandover();
            return;
          }
          this.busy = false;
          this.turnsEnded++;
          // The turn ended while a message was kept back: it goes now, as the next message.
          if (event.session.id === this.selectedId && event.session.status !== "failed") this.#flushHeld();
        }
        return;
      }
      if (id !== this.selectedId) return;
      if (event.kind === "line") {
        if (event.line.includes('"qanuvi_interrupted"')) this.interrupting = false;
        this.items = reduce(this.items, event.line, ++this.#seq, Math.floor(Date.now() / 1000));
        // Each model call's size is in its answer (no extra reads): past the limit, the chat offers a handover.
        if (event.line.includes('"usage"') && event.line.includes('"assistant"')) {
          const size = contextSize([event.line]);
          if (size) {
            this.context = size;
            this.#checkHandover();
          }
        }
        // An approval answered by a standing one: the minutes left change.
        if (event.line.includes('"auto":true')) this.loadGrants();
      } else if (event.kind === "error") {
        // An older Claude Code that does not list a newer model yet still uses it: a quiet note, not an error.
        if (event.text.includes("unrecognized_model")) {
          if (!this.items.some((i) => i.kind === "notice" && i.text.startsWith("This Claude Code does not list")))
            this.notice(
              "This Claude Code does not list this model yet, but it uses it. Updating Claude Code removes this note.",
            );
        } else this.errors = [...this.errors.slice(-20), event.text];
      } else if (event.kind === "exit") this.busy = false;
    };
  }

  /**
   * `note`: words Qanuvi adds for the agent after the user's (not shown as theirs); `before`: words it puts in front
   * (a branch's earlier conversation, 24n part 6), not shown either.
   */
  async send(
    projectId: string,
    prompt: string,
    note = "",
    before = "",
    opts: { noHold?: boolean; files?: Attachment[] } = {},
  ): Promise<boolean> {
    const text = prompt.trim();
    if (!text) return false;
    this.error = "";
    // A reopened starred chat (24s part 3): the message goes to a fresh session that starts from the note; with no
    // note yet, the old session writes it once now and the message waits for the new session.
    const r = this.reopened;
    if (r && this.selectedId === r.id && !note && !before && !this.handover) {
      const from = this.selected;
      this.reopened = null;
      if (from && r.note !== null) {
        forgetNote(localStorage, r.id);
        this.busy = true;
        await this.#goOnFresh(from, r.note, "", r.was, [text]);
        return true;
      }
      if (from) await this.handOver(true);
    }
    // During a handover the message waits and goes to the new session (24s).
    if (this.handover && this.selectedId === this.handover.fromId && !note && !before) {
      this.handover.queued.push(text);
      this.items = reduce(
        this.items,
        JSON.stringify({ type: "qanuvi_user", text, during: true }),
        ++this.#seq,
        nowSeconds(),
      );
      return true;
    }
    // Sent while the agent works: it joins the task (the agent takes it in before its next step).
    const during = !!this.selectedId && (this.busy || ["running", "waiting"].includes(this.selected?.status ?? ""));
    // Kept back a few seconds first: "Don't send" can still drop it (files go at once, they are not kept back).
    if (during && !opts.noHold && !note && !before && !this.attachments.length && HOLD_MS > 0) {
      this.items = reduce(
        this.items,
        JSON.stringify({ type: "qanuvi_user", text, during: true, held: true }),
        ++this.#seq,
        nowSeconds(),
      );
      const id = this.items[this.items.length - 1].id;
      this.#held.set(id, { projectId, text, timer: setTimeout(() => void this.sendHeld(id), HOLD_MS) });
      return true;
    }
    this.busy = true;
    // The chat the message was written in, also when another one is opened before it goes out.
    const target = this.selectedId;
    let currentId = target;
    // Show the message straight away; the stored copy is the same text.
    const files = opts.files ?? this.attachments;
    this.items = reduce(
      this.items,
      JSON.stringify({
        type: "qanuvi_user",
        text,
        ...(files.length ? { attachments: files.map((f) => f.name) } : {}),
        // Images show straight away from the data in hand; the stored line names the kept copies.
        ...(files.some((f) => f.kind === "image")
          ? {
              images: files
                .filter((f) => f.kind === "image")
                .map((f) => ({ name: f.name, src: `data:${f.mediaType};base64,${f.data}` })),
            }
          : {}),
        ...(during ? { during: true } : {}),
      }),
      ++this.#seq,
      nowSeconds(),
    );
    try {
      // A new session gets the relevant part of the user's memory with its first message.
      const context = target ? null : await memoryContext(projectId, text);
      // Free decides, paid works: a free model picks the paid agent's model for this message.
      let model = this.model;
      let sent = before + text + note;
      // Paid agent on top (24e): what a free model changes in chat is reviewed by the paid one.
      if (!during) this.freeTurn = null;
      if (!during && this.providerId === "qanuvi" && this.model === "auto" && this.info?.available && paidAgent()) {
        const mode = settings.value.team.mode;
        if (mode === "paid-decides" || mode === "priority")
          this.freeTurn = { projectId, request: text, before: await snapshotChanges(projectId) };
      }
      this.freePicked = null;
      if (during) {
        // The running session keeps its model; the message only joins the task.
        model = this.selected?.model || this.model;
      } else if (this.#forceModel) {
        // The paid agent overruled the free pick: this message goes to the model it asked for.
        model = this.#forceModel;
        this.#forceModel = null;
      } else if (this.providerId !== "qanuvi" && this.model === "auto") {
        const pick = await this.#freePick(text);
        model = pick.model;
        // The paid agent sits on top (24e): it sees the pick and can overrule it.
        if (pick.model !== providerById(this.providerId).models[0]?.id) {
          sent = before + text + overruleNote(modelLabel(providerById(this.providerId), pick.model), pick.why);
          this.freePicked = { text, model: pick.model };
        }
      }
      // Withdrawn after the agent had it: this message tells the agent to leave it.
      const gone = target ? this.#withdrawn.get(target) : undefined;
      if (target && gone?.length) {
        sent += withdrawnNote(gone);
        this.#withdrawn.delete(target);
      }
      const session = await api.agent.start(
        projectId,
        target,
        sent,
        model,
        // A message while it works keeps the running session's mode (a plan run's Auto stays Auto).
        during ? this.selected?.mode || this.mode : this.mode,
        settings.agent(this.providerId).baseUrl,
        this.effort,
        this.handler(() => currentId),
        false,
        settings.agent(this.providerId).indexTools,
        settings.agent(this.providerId).browserTools,
        context,
        this.providerId,
        setupFor(this.providerId),
        files,
      );
      // Sent with this message: the next one starts without them.
      if (this.attachments === files) this.attachments = [];
      currentId = session.id;
      this.#upsert(session);
      if (this.selectedId === target) this.selectedId = session.id;
      return true;
    } catch (e) {
      this.error = errorText(e);
      this.busy = false;
      return false;
    }
  }

  /**
   * Free decides, paid works (24b): Qanuvi core's free models, online ones first, pick the paid agent's model for a
   * message. The pick and its reason show in the conversation; when no free model answers, the agent's default is used.
   */
  async #freePick(text: string): Promise<{ model: string; why: string }> {
    const provider = providerById(this.providerId);
    const fallback = provider.defaultModel;
    const def = { model: fallback, why: "" };
    if (!ownSetup().models.length) return def;
    const note = (t: string, model: string) =>
      (this.items = reduce(
        this.items,
        JSON.stringify({ type: "system", subtype: "qanuvi_route", text: t, model }),
        ++this.#seq,
        nowSeconds(),
      ));
    try {
      const { by, text: answer } = await freeDecide(
        "You choose models for a coding assistant. Answer with JSON only.",
        chatModelPrompt(text, provider.models),
      );
      const pick = parseChatModel(answer, provider.models);
      const model = pick?.model ?? fallback;
      note(
        `Free decides: ${by} picked ${modelLabel(provider, model)}${pick?.why ? ` (${pick.why})` : " (its answer could not be used; the default)"}.`,
        model,
      );
      return { model, why: pick?.why ?? "" };
    } catch (e) {
      note(
        `Free decides: no free model answered (${errorText(e)}); ${modelLabel(provider, fallback)} works on it.`,
        fallback,
      );
      return def;
    }
  }

  /** A free chat turn the paid agent reviews when it ends: the request and the changed files before it. */
  freeTurn: { projectId: string; request: string; before: Map<string, string> } | null = null;

  /** Adds a line from Qanuvi core to the conversation. */
  notice(text: string, model = ""): void {
    this.items = reduce(
      this.items,
      JSON.stringify({ type: "system", subtype: "qanuvi_route", text, model }),
      ++this.#seq,
      nowSeconds(),
    );
  }

  /** The message whose model a free model picked (the paid agent may overrule it), until the next message. */
  freePicked = $state<{ text: string; model: string } | null>(null);
  #forceModel: string | null = null;

  /**
   * The paid agent answered that the free pick is too weak (24e): the same message goes again, in the same session,
   * to one model up. Once per message.
   */
  async overrule(projectId: string, why: string): Promise<boolean> {
    const picked = this.freePicked;
    if (!picked) return false;
    this.freePicked = null;
    const provider = providerById(this.providerId);
    const model = strongerModel(provider.models, picked.model);
    this.items = reduce(
      this.items,
      JSON.stringify({
        type: "system",
        subtype: "qanuvi_route",
        text: `${provider.name} overruled the free pick (${why}); ${modelLabel(provider, model)} works on it.`,
        model,
      }),
      ++this.#seq,
      nowSeconds(),
    );
    this.#forceModel = model;
    return this.send(projectId, picked.text);
  }

  /**
   * A free model got into trouble: the paid agent takes the request over in a new session, with what the free one did
   * (24b). Never for a model the user picked: that one is used alone.
   */
  async handToPaid(projectId: string, paidId = paidAgent()?.id ?? ""): Promise<boolean> {
    if (!paidId) return false;
    const lastUser = [...this.items].reverse().find((i) => i.kind === "user");
    if (!lastUser || lastUser.kind !== "user") return false;
    const tried = itemsToMarkdown("What a free model did", this.items).slice(-5000);
    const prompt = `${lastUser.text}\n\n(A free model worked on this first and got into trouble. What it did, for your reference:)\n\n${tried}`;
    const a = settings.agent(paidId);
    this.selectedId = null;
    this.items = [];
    this.errors = [];
    this.providerId = paidId;
    this.model = a.model;
    this.mode = a.mode;
    this.effort = a.effort;
    return this.send(projectId, prompt);
  }

  /**
   * The paid agent hit its usage limit: the work goes on with Qanuvi's own agent and the free models, given the request
   * and what was done so far.
   */
  async handToFree(projectId: string): Promise<boolean> {
    if (!settings.value.brain.models.length) return false;
    const lastUser = [...this.items].reverse().find((i) => i.kind === "user");
    if (!lastUser || lastUser.kind !== "user") return false;
    const done = itemsToMarkdown("What the paid agent did before its limit", this.items).slice(-5000);
    const prompt = `${lastUser.text}\n\n(The paid agent worked on this first and hit its usage limit. Go on from where it stopped; what it did, for reference:)\n\n${done}`;
    const a = settings.agent("qanuvi");
    this.selectedId = null;
    this.items = [];
    this.errors = [];
    this.providerId = "qanuvi";
    this.model = a.model;
    this.mode = a.mode;
    this.effort = a.effort;
    return this.send(projectId, prompt);
  }

  /** Shows a session that something else (a plan run) is starting: empty pane now, its id once known. */
  watch(sessionId: string | null): void {
    if (sessionId === null) {
      this.newSession();
      this.busy = true;
      return;
    }
    this.selectedId = sessionId;
  }

  /** Records a session started elsewhere (a plan run) so it shows in the list. */
  track(session: Session): void {
    this.#upsert(session);
  }

  async loadGrants(): Promise<void> {
    const id = this.selectedId;
    const list = id ? await api.agent.grants(id).catch(() => []) : [];
    if (id === this.selectedId) this.grants = list;
  }

  async forgetGrants(): Promise<void> {
    if (!this.selectedId) return;
    await api.agent.forgetGrants(this.selectedId).catch(() => {});
    this.grants = [];
  }

  /** remember: "" (this one), "session", or "minutes:N". */
  async respond(requestId: string, allow: boolean, remember = ""): Promise<void> {
    if (!this.selectedId) return;
    try {
      await api.agent.respond(this.selectedId, requestId, allow, remember);
      if (remember) this.loadGrants();
      this.items = reduce(
        this.items,
        JSON.stringify({ type: "qanuvi_permission", request_id: requestId, allow }),
        ++this.#seq,
        nowSeconds(),
      );
    } catch (e) {
      this.error = errorText(e);
    }
  }

  /**
   * Edit and resend (24n part 6): a new session goes on from before the user message at `index`, with the message as
   * changed. It is told what was said before (every agent can take that); the original session stays as it was.
   * `rewound`: the file changes from that message on were undone first.
   */
  async branch(projectId: string, index: number, edited: string, rewound: boolean): Promise<boolean> {
    const from = this.selected;
    if (!from || this.busy) return false;
    const preamble = branchPreamble(this.items.slice(0, index), rewound);
    // The branch keeps the agent, model, mode and effort of the session it comes from.
    this.selectedId = null;
    this.items = [];
    this.errors = [];
    this.grants = [];
    const ok = await this.send(projectId, edited, "", preamble);
    if (!ok) {
      await this.select(from.id);
      return false;
    }
    if (this.selectedId) await this.rename(this.selectedId, `${from.title} (branch)`).catch(() => {});
    return true;
  }

  /** "Stop this command, read my message now" asked and not answered yet. */
  interrupting = $state(false);

  /**
   * Ends the agent's turn and its command, keeps the session: the waiting message is read next (24r). A plan step
   * hears it too, so the turn's end is not taken for the step's end.
   */
  async interrupt(): Promise<void> {
    const id = this.selectedId;
    if (!id || this.interrupting) return;
    this.interrupting = true;
    try {
      // Messages still kept back go first: the agent reads them next.
      await Promise.all(this.#flushHeld());
      if (id === this.planSessionId) this.onPlanInterrupt?.();
      await api.agent.interrupt(id);
    } catch (e) {
      this.error = errorText(e);
      this.interrupting = false;
    }
  }

  /** Sends a kept-back message now ("Send now", or its few seconds are up). */
  sendHeld(id: string): Promise<boolean> {
    const h = this.#held.get(id);
    if (!h) return Promise.resolve(false);
    clearTimeout(h.timer);
    this.#held.delete(id);
    this.items = this.items.filter((i) => i.id !== id);
    return this.send(h.projectId, h.text, "", "", { noHold: true, files: [] });
  }

  /** "Don't send" on a kept-back message: it never reaches the agent. */
  dropHeld(id: string): void {
    const h = this.#held.get(id);
    if (!h) return;
    clearTimeout(h.timer);
    this.#held.delete(id);
    this.items = this.items.filter((i) => i.id !== id);
  }

  /**
   * "Don't send" on a message the agent already started a turn for (the turn had ended before it took the message
   * in): that turn stops, and the next message tells the agent to leave it. A message the agent has cannot be pulled
   * back from it, only stopped and countermanded.
   */
  async withdraw(id: string): Promise<void> {
    const sid = this.selectedId;
    const item = this.items.find((i) => i.id === id);
    if (!sid || item?.kind !== "user" || item.waiting !== "turn") return;
    this.#withdrawn.set(sid, [...(this.#withdrawn.get(sid) ?? []), item.text]);
    this.items = this.items.map((i) =>
      i.id === id && i.kind === "user" ? { ...i, waiting: undefined, withdrawn: true } : i,
    );
    await this.interrupt();
  }

  /** Every kept-back message goes now (another chat opens, the turn ended, or "read my message now"). */
  #flushHeld(): Promise<boolean>[] {
    if (!this.#held.size) return [];
    // Called from effects too (newSession): what the sends read must not become the effect's dependencies.
    return untrack(() => [...this.#held.keys()].map((id) => this.sendHeld(id)));
  }

  /** The runner hears an interrupt on its step here. */
  onPlanInterrupt: (() => void) | null = null;

  async stop(): Promise<void> {
    if (!this.selectedId) return;
    await api.agent.stop(this.selectedId).catch(() => {});
    this.busy = false;
  }

  /** Deletes a chat: the session and the earlier ones it goes on from (24s). */
  async remove(id: string): Promise<void> {
    if (this.sessions.find((s) => s.id === id)?.saved) {
      this.error = "This chat is starred. Unstar it first to delete it.";
      return;
    }
    const ids = chainOf(this.sessions, id).map((s) => s.id);
    if (!ids.includes(id)) ids.push(id);
    for (const x of ids) await api.agent.remove(x).catch((e) => (this.error = errorText(e)));
    this.sessions = this.sessions.filter((s) => !ids.includes(s.id));
    if (this.selectedId && ids.includes(this.selectedId)) this.newSession();
  }

  /** Stars or unstars a chat (24s part 2): starred chats sit under Saved and are not deleted while starred. */
  async star(id: string, saved: boolean): Promise<void> {
    try {
      await api.agent.saved(id, saved);
      const s = this.sessions.find((x) => x.id === id);
      if (s) s.saved = saved;
    } catch (e) {
      this.error = errorText(e);
    }
  }

  async rename(id: string, title: string): Promise<void> {
    try {
      await api.agent.rename(id, title);
      const s = this.sessions.find((x) => x.id === id);
      if (s) s.title = title.trim();
    } catch (e) {
      this.error = errorText(e);
    }
  }
}

export const agent = new AgentStore();
