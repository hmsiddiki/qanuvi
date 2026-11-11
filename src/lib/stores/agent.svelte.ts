import { api, errorText, type AgentEvent, type AgentInfo, type Attachment, type Grant, type Session } from "$lib/api";
import { reduce, reduceAll, type Item } from "$lib/agent/events";
import { settings } from "./settings.svelte";
import { memoryContext } from "./memory.svelte";
import { modelLabel, paidAgent, providerById, type AgentProvider } from "$lib/agents/providers";
import { chatModelPrompt, deciderSeat, overruleNote, parseChatModel, strongerModel } from "$lib/core/decider";
import { itemsToMarkdown } from "$lib/agent/events";
import { freeDecide, ownSetup, setupFor } from "./brain.svelte";
import { snapshotChanges } from "./changes";

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

  newSession(): void {
    this.selectedId = null;
    this.items = [];
    this.errors = [];
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
    this.selectedId = id;
    this.loadGrants();
    this.errors = [];
    const s = this.selected;
    if (s) {
      this.providerId = s.agent;
      this.model = s.model;
      this.mode = s.mode;
    }
    try {
      const lines = await api.agent.events(id);
      if (this.selectedId === id) {
        this.items = reduceAll(lines);
        this.#seq = lines.length;
      }
    } catch (e) {
      this.error = errorText(e);
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
        if (["idle", "ended", "failed", "stopped"].includes(event.session.status)) {
          this.busy = false;
          this.turnsEnded++;
        }
        return;
      }
      if (id !== this.selectedId) return;
      if (event.kind === "line") {
        this.items = reduce(this.items, event.line, ++this.#seq);
        // An approval answered by a standing one: the minutes left change.
        if (event.line.includes('"auto":true')) this.loadGrants();
      } else if (event.kind === "error") this.errors = [...this.errors.slice(-20), event.text];
      else if (event.kind === "exit") this.busy = false;
    };
  }

  async send(projectId: string, prompt: string): Promise<boolean> {
    const text = prompt.trim();
    if (!text) return false;
    this.error = "";
    // Sent while the agent works: it joins the task (the agent takes it in before its next step).
    const during = !!this.selectedId && (this.busy || ["running", "waiting"].includes(this.selected?.status ?? ""));
    this.busy = true;
    let currentId = this.selectedId;
    // Show the message straight away; the stored copy is the same text.
    const files = this.attachments;
    this.items = reduce(
      this.items,
      JSON.stringify({
        type: "qanuvi_user",
        text,
        ...(files.length ? { attachments: files.map((f) => f.name) } : {}),
        ...(during ? { during: true } : {}),
      }),
      ++this.#seq,
    );
    try {
      // A new session gets the relevant part of the user's memory with its first message.
      const context = this.selectedId ? null : await memoryContext(projectId, text);
      // Free decides, paid works: a free model picks the paid agent's model for this message.
      let model = this.model;
      let sent = text;
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
          sent = text + overruleNote(modelLabel(providerById(this.providerId), pick.model), pick.why);
          this.freePicked = { text, model: pick.model };
        }
      }
      const session = await api.agent.start(
        projectId,
        this.selectedId,
        sent,
        model,
        this.mode,
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
      this.selectedId = session.id;
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
      );
    } catch (e) {
      this.error = errorText(e);
    }
  }

  async stop(): Promise<void> {
    if (!this.selectedId) return;
    await api.agent.stop(this.selectedId).catch(() => {});
    this.busy = false;
  }

  async remove(id: string): Promise<void> {
    await api.agent.remove(id).catch((e) => (this.error = errorText(e)));
    this.sessions = this.sessions.filter((s) => s.id !== id);
    if (this.selectedId === id) this.newSession();
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
