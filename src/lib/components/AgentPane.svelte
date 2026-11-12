<script lang="ts">
  import { contextMenu, copyText } from "$lib/stores/contextMenu.svelte";
  import AttachMenu from "./AttachMenu.svelte";
  import { clipboardFiles, pastedAttachments } from "$lib/agent/paste";
  import ChatImage from "./ChatImage.svelte";
  import { overruleIn } from "$lib/core/decider";
  import { reviewFreeTurn } from "$lib/stores/chatReview";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import Square from "@lucide/svelte/icons/square";
  import Star from "@lucide/svelte/icons/star";
  import Bot from "@lucide/svelte/icons/bot";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import CircleX from "@lucide/svelte/icons/circle-x";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import ShieldQuestion from "@lucide/svelte/icons/shield-question";
  import Brain from "@lucide/svelte/icons/brain";
  import Circle from "@lucide/svelte/icons/circle";
  import ListChecks from "@lucide/svelte/icons/list-checks";
  import StepCard from "./StepCard.svelte";
  import MentionPicker, { type Choice } from "./MentionPicker.svelte";
  import { inProject, insertMention, isImagePath, mentionAt } from "$lib/agent/mention";
  import { revertEdits, type Edit } from "$lib/agent/diff";
  import { matchesIn, rewindFrom, rewindText, rowsToOpen, searchItems } from "$lib/agent/sessionTools";
  import { tokensK } from "$lib/agent/handover";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Undo2 from "@lucide/svelte/icons/undo-2";
  import ChevronUp from "@lucide/svelte/icons/chevron-up";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import XIcon from "@lucide/svelte/icons/x";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { api, errorText } from "$lib/api";
  import { tick, untrack } from "svelte";
  import { agent } from "$lib/stores/agent.svelte";
  import { projects } from "$lib/stores/projects.svelte";
  import { editor } from "$lib/stores/editor.svelte";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import {
    clockTime,
    currentModel,
    formatCost,
    formatTokens,
    looksLikeCall,
    timelineRows,
    type Item,
    type Row,
  } from "$lib/agent/events";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Search from "@lucide/svelte/icons/search";
  import { renderMarkdown } from "$lib/agent/markdown";
  import { openUrl } from "@tauri-apps/plugin-opener";

  /** Links in an answer open in the user's browser, not inside Qanuvi. */
  function openLink(e: MouseEvent) {
    const a = (e.target as HTMLElement).closest("a");
    if (!a) return;
    e.preventDefault();
    const href = a.getAttribute("href") ?? "";
    if (/^https?:\/\//.test(href)) openUrl(href);
  }
  import { modelLabel, paidAgent } from "$lib/agents/providers";
  import { layout } from "$lib/layout/layout.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { wizard } from "$lib/stores/wizard.svelte";

  /** No paid agent and no free models yet: Qanuvi's own agent can be set up for free. */
  function setUpFreeAi() {
    settings.set("agent", "qanuvi");
    agent.newSession();
    wizard.open = true;
    ui.show("agents");
  }
  import ModePicker from "./ModePicker.svelte";
  import { ask } from "$lib/stores/confirm.svelte";
  import { guidanceNote, isStopOrder, looksSecret } from "$lib/plan/guidance";
  import { isLimit } from "$lib/plan/limits";
  import { runner } from "$lib/stores/runner.svelte";
  import CommandMenu from "./CommandMenu.svelte";

  let commandsOpen = $state(false);

  // @ mentions: the "@query" the caret is in, and the picker's choices.
  let mention = $state<{ start: number; query: string } | null>(null);
  let mentionChoices = $state<Choice[]>([]);
  let mentionSelected = $state(0);
  function findMention() {
    const box = agent.input;
    mention = box && box.selectionStart === box.selectionEnd ? mentionAt(agent.draft, box.selectionStart) : null;
  }
  async function pickMention(c: Choice) {
    const box = agent.input;
    if (!mention || !box) return;
    const done = insertMention(agent.draft, mention.start, box.selectionStart, c.mention);
    agent.draft = done.text;
    mention = null;
    await tick();
    box.focus();
    box.setSelectionRange(done.caret, done.caret);
  }

  // Files dropped onto the message box (from the file manager): images are attached, files of the project are
  // mentioned (the agent reads them itself, only as much as it needs), other files are attached.
  let composer = $state<HTMLFormElement>();
  let dropping = $state(false);
  function overComposer(x: number, y: number): boolean {
    const r = composer?.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    return !!r && x / dpr >= r.left && x / dpr <= r.right && y / dpr >= r.top && y / dpr <= r.bottom;
  }
  async function dropped(paths: string[]) {
    const p = projects.current;
    if (!p) return;
    const mentions: string[] = [];
    const errors: string[] = [];
    for (const path of paths) {
      const rel = inProject(p.path, path);
      if (rel && !isImagePath(path)) {
        mentions.push(`@${rel}`);
        continue;
      }
      try {
        agent.attachments = [...agent.attachments, await api.attachments.file(path)];
      } catch (e) {
        errors.push(errorText(e));
      }
    }
    if (mentions.length) {
      const sep = agent.draft && !/\s$/.test(agent.draft) ? " " : "";
      agent.draft = `${agent.draft}${sep}${mentions.join(" ")} `;
    }
    agent.error = errors.join(" ");
    agent.input?.focus();
  }
  $effect(() => {
    let stop: (() => void) | null = null;
    let gone = false;
    getCurrentWebview()
      .onDragDropEvent((e) => {
        const d = e.payload;
        if (d.type === "leave") dropping = false;
        else if (d.type === "enter" || d.type === "over")
          dropping = !!projects.current && overComposer(d.position.x, d.position.y);
        else if (d.type === "drop") {
          const over = overComposer(d.position.x, d.position.y);
          dropping = false;
          if (over && d.paths.length) void dropped(d.paths);
        }
      })
      .then((u) => (gone ? u() : (stop = u)))
      .catch(() => {});
    return () => {
      gone = true;
      stop?.();
    };
  });

  /** Right-click in the message box: cut, copy, and paste that also takes images (like Ctrl+V). */
  function boxMenu(e: MouseEvent) {
    const box = agent.input;
    const picked = box ? agent.draft.slice(box.selectionStart, box.selectionEnd) : "";
    contextMenu.show(e, [
      [
        {
          label: "Cut",
          hint: "Ctrl+X",
          disabled: !picked,
          action: () => {
            copyText(picked);
            if (box) agent.draft = agent.draft.slice(0, box.selectionStart) + agent.draft.slice(box.selectionEnd);
          },
        },
        { label: "Copy", hint: "Ctrl+C", disabled: !picked, action: () => copyText(picked) },
        { label: "Paste", hint: "Ctrl+V", action: () => pasteFromMenu() },
      ],
    ]);
  }
  async function pasteFromMenu() {
    const box = agent.input;
    try {
      const items = await navigator.clipboard.read();
      const files: File[] = [];
      let text = "";
      for (const item of items) {
        const type = item.types.find((t) => /^image\/(png|jpeg|gif|webp)$/.test(t));
        if (type) files.push(new File([await item.getType(type)], "image.png", { type }));
        else if (item.types.includes("text/plain")) text += await (await item.getType("text/plain")).text();
      }
      if (files.length) {
        const images = agent.attachments.filter((a) => a.kind === "image").length;
        const got = await pastedAttachments(files, images);
        agent.attachments = [...agent.attachments, ...got.attachments];
        agent.error = got.errors.join(" ");
      } else if (text && box) {
        const at = box.selectionStart;
        agent.draft = agent.draft.slice(0, at) + text + agent.draft.slice(box.selectionEnd);
        await tick();
        box.setSelectionRange(at + text.length, at + text.length);
      }
      box?.focus();
    } catch {
      agent.error = "The clipboard could not be read here; press Ctrl+V in the message box instead.";
    }
  }

  // Paid decides, free works (24b): when the free models fail a request (with Auto, not a model the user picked), the
  // paid agent takes it over by itself. Once per session.
  const handedOver = new Set<string>();
  $effect(() => {
    const last = agent.items[agent.items.length - 1];
    const id = agent.selectedId;
    if (!last || last.kind !== "result" || last.ok || !id || handedOver.has(id)) return;
    const team = settings.value.team;
    if (
      agent.provider.id === "qanuvi" &&
      agent.model === "auto" &&
      agent.info?.available &&
      (team.mode === "paid-decides" || team.mode === "priority") &&
      projects.current
    ) {
      handedOver.add(id);
      // Untracked: what the hand-over changes must never re-run this.
      const pid = projects.current.id;
      untrack(() => agent.handToPaid(pid));
    }
  });

  // Paid agent on top (24e): a free chat turn that ended well and changed files is reviewed by the paid agent.
  $effect(() => {
    const last = agent.items[agent.items.length - 1];
    if (!last || last.kind !== "result" || !agent.freeTurn || !projects.current) return;
    const p = projects.current;
    untrack(() => {
      if (last.ok) reviewFreeTurn(p);
      else agent.freeTurn = null;
    });
  });

  // Free decides, paid works, with the paid agent on top (24e): its answer can overrule the free pick.
  $effect(() => {
    const last = agent.items[agent.items.length - 1];
    if (!last || last.kind !== "result" || !agent.freePicked || !projects.current) return;
    const answer = [...agent.items].reverse().find((i) => i.kind === "text");
    const why = answer && answer.kind === "text" ? overruleIn(answer.text) : null;
    const pid = projects.current.id;
    untrack(() => {
      if (why) agent.overrule(pid, why);
      else agent.freePicked = null;
    });
  });

  function openAgentSettings() {
    if (!layout.state.workVisible) layout.toggleWork();
    ui.show("agents");
  }

  let scroller: HTMLDivElement;

  /** Groups and folds the user opened (by row id; a new session starts folded). */
  let opened = $state(new Set<string>());
  $effect(() => {
    void agent.selectedId;
    opened = new Set();
  });
  function toggle(id: string) {
    const next = new Set(opened);
    if (!next.delete(id)) next.add(id);
    opened = next;
  }

  let project = $derived(projects.current);
  let session = $derived(agent.selected);
  let running = $derived(agent.busy || session?.status === "running" || session?.status === "waiting");
  /** The model at work, as the agent named it (Qanuvi's brain names each pick and handover). */
  let working = $derived(currentModel(agent.items));
  /** Messages sent while the agent works that it has not read yet (24r): shown above the box, not in the timeline. */
  /** A waiting message the agent took in only after its turn ended: it is reading it in a turn of its own. */
  let readingNow = $derived(agent.items.some((i) => i.kind === "user" && i.waiting === "turn" && !i.older));
  let waitingMsgs = $derived(
    running
      ? agent.items.filter((i): i is Extract<Item, { kind: "user" }> => i.kind === "user" && !!i.waiting && !i.older)
      : [],
  );
  /** The conversation as timeline rows: look-around steps grouped, long runs folded. */
  let rows = $derived(
    timelineRows(
      waitingMsgs.length ? agent.items.filter((i) => !(i.kind === "user" && waitingMsgs.includes(i))) : agent.items,
    ),
  );

  $effect(() => {
    void agent.items.length;
    tick().then(() => scroller && (scroller.scrollTop = scroller.scrollHeight));
  });

  async function send(event?: SubmitEvent) {
    event?.preventDefault();
    // While the agent works the message joins the task: the agent reads it before its next step.
    if (!project || !agent.draft.trim()) return;
    const text = agent.draft;
    agent.draft = "";
    // The step a plan run is working on now: the user is watching and guides it.
    const step =
      runner.sessionId && runner.sessionId === session?.id
        ? runner.run?.steps.find((s) => s.status === "running")
        : null;
    // A step the last run stopped at: writing in its session resumes the run from it (not a plain chat message).
    if (!step && session && !running && /^Step [^:\s]+:/.test(session.title) && !isStopOrder(text)) {
      if (looksSecret(text) && runner.run) await runner.addPrivate(project.id, runner.run.file, text);
      else runner.addNote(text);
      if (await runner.resumeFrom(project, session.id, text)) {
        agent.notice("The plan run goes on from this step, in this session, with your message first.");
        return;
      }
    }
    if (step && isStopOrder(text)) {
      runner.stop();
      agent.notice("You said stop: the plan run stops here. Run it again to go on.");
      return;
    }
    if (step && looksSecret(text)) {
      const asSecret = await ask(
        "This looks like a login or password. It will be kept for this plan, so every later step and run of it has it (in Windows' credential store, never in files). Better still: a project secret reaches every step and the tests as an environment variable, without any model seeing it.",
        {
          title: "A password in the chat",
          kind: "warning",
          okLabel: "Open project secrets",
          cancelLabel: "Use it for this plan",
        },
      );
      if (asSecret) {
        agent.draft = text;
        ui.settingsFor = project.id;
        return;
      }
      if (runner.run) await runner.addPrivate(project.id, runner.run.file, text);
    } else if (step) runner.addNote(text);
    const note = step && runner.run ? guidanceNote(step.number ?? step.title, runner.run.file) : "";
    if (!(await agent.send(project.id, text, note))) agent.draft = text;
  }

  function keydown(event: KeyboardEvent) {
    if (mention && mentionChoices.length) {
      const n = mentionChoices.length;
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        mentionSelected = (mentionSelected + (event.key === "ArrowDown" ? 1 : n - 1)) % n;
        return;
      }
      if ((event.key === "Enter" && !event.shiftKey) || event.key === "Tab") {
        event.preventDefault();
        void pickMention(mentionChoices[mentionSelected]);
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        mention = null;
        return;
      }
    }
    // Enter with the menu open puts it away; it never sends a lone "/".
    if (commandsOpen && event.key === "Enter") {
      event.preventDefault();
      commandsOpen = false;
      return;
    }
    if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
      event.preventDefault();
      send();
    } else if (event.key === "Tab" && event.shiftKey && !running) {
      event.preventDefault();
      agent.cycleMode();
    } else if (commandsOpen && event.key !== "/") {
      // The menu came from typing "/": any other key (space, arrows, Escape, more text) puts it away again.
      commandsOpen = false;
    }
  }

  /** "/" as the whole message opens the menu (read from the text, so every keyboard layout works). */
  function input() {
    findMention();
    if (agent.draft === "/") commandsOpen = true;
  }

  /** What a message attached besides its images (those show as pictures). */
  function otherFiles(item: { attachments?: string[]; images?: { name: string }[] }): string[] {
    return (item.attachments ?? []).filter((n) => !item.images?.some((im) => im.name === n));
  }

  /** Ctrl+V with an image (a screenshot) or copied files: they become attachments; plain text pastes as usual. */
  async function paste(event: ClipboardEvent) {
    const files = clipboardFiles(event.clipboardData);
    if (!files.length) return;
    event.preventDefault();
    const images = agent.attachments.filter((a) => a.kind === "image").length;
    const got = await pastedAttachments(files, images);
    if (got.attachments.length) agent.attachments = [...agent.attachments, ...got.attachments];
    agent.error = got.errors.join(" ");
  }

  /** Opens a file an agent step worked on, when it is inside the project. */
  /** Right-click on a message: copy it, or quote it in the reply. */
  function messageMenu(e: MouseEvent, text: string, userId = "") {
    const picked = window.getSelection()?.toString() ?? "";
    const own = userId && session && !running;
    contextMenu.show(e, [
      own
        ? [
            { label: "Edit and send again (a branch)", action: () => startEdit(userId, text) },
            { label: "Rewind the files to before this message", action: () => void rewindTo(userId) },
          ]
        : [],
      [
        picked && { label: "Copy the selection", hint: "Ctrl+C", action: () => copyText(picked) },
        { label: "Copy the message", action: () => copyText(text) },
      ],
      [
        {
          label: "Quote in reply",
          action: () => {
            const quote = (picked || text)
              .trim()
              .split("\n")
              .map((l) => `> ${l}`)
              .join("\n");
            agent.draft = `${quote}\n\n${agent.draft}`;
            agent.input?.focus();
          },
        },
      ],
    ]);
  }

  /** Revert from the chat: the step's edits undone in the file, when the file still has them as the step left them. */
  async function revertStep(step: { file: string | null; edits?: Edit[]; whole?: boolean }): Promise<string | null> {
    const p = projects.current;
    if (!p || !step.file || !step.edits || step.whole) return "This change cannot be undone from the chat.";
    const rel = inProject(p.path, step.file);
    if (!rel) return "The file is outside the project.";
    const now = await api.files.read(p.id, rel).catch(() => null);
    if (!now) return "The file could not be read.";
    const back = revertEdits(now.text, step.edits);
    if (back === null)
      return "The file has changed since this step (or the changed text is there more than once), so it is not undone here; Source control can undo it.";
    if (!(await ask(`Undo this change in ${rel}?`, { title: "Revert the change", okLabel: "Revert" }))) return "";
    try {
      await api.files.write(p.id, rel, back, now.hash);
    } catch (e) {
      return errorText(e);
    }
    return null;
  }

  // Edit and resend (24n part 6): the message as changed goes to a new session (a branch) that is told what was said
  // before it; the original session stays as it was. The file changes made from it on can be undone first.
  let editing = $state<{ id: string; text: string; rewind: boolean } | null>(null);
  const indexOfItem = (id: string) => agent.items.findIndex((i) => i.id === id);
  function startEdit(id: string, text: string) {
    editing = { id, text, rewind: false };
  }
  /** File-changing steps from a message on, for the edit box's checkbox. */
  function changesFrom(id: string): { steps: number; files: number } {
    const i = indexOfItem(id);
    if (i < 0) return { steps: 0, files: 0 };
    const r = rewindFrom(agent.items, i);
    return { steps: r.steps, files: r.files.length + r.cannot.length };
  }
  async function resend() {
    const e = editing;
    if (!e || !project || running || !e.text.trim()) return;
    const i = indexOfItem(e.id);
    if (i < 0) return;
    let rewound = false;
    if (e.rewind) {
      const r = await rewindTo(e.id);
      if (r === "cancel") return;
      rewound = r === "done";
    }
    editing = null;
    await agent.branch(project.id, i, e.text.trim(), rewound);
  }
  function editKeys(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      editing = null;
    } else if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
      event.preventDefault();
      void resend();
    }
  }

  /**
   * Rewind (24n part 6): the Edit/Write steps from a message on undone in their files, newest first, after a preview.
   * A file changed since, or written whole by a step, is left as it is and named.
   */
  async function rewindTo(id: string): Promise<"done" | "cancel" | "nothing"> {
    const p = projects.current;
    const i = indexOfItem(id);
    if (!p || i < 0) return "nothing";
    const plan = rewindFrom(agent.items, i);
    if (!plan.files.length && !plan.cannot.length) {
      agent.notice("No file changes from this message on: nothing to rewind.");
      return "nothing";
    }
    const ready: { rel: string; text: string; hash: string }[] = [];
    const left = plan.cannot.map((c) => `${inProject(p.path, c.file) ?? c.file}: ${c.why}`);
    for (const f of plan.files) {
      const rel = inProject(p.path, f.file);
      if (!rel) {
        left.push(`${f.file}: outside the project`);
        continue;
      }
      const now = await api.files.read(p.id, rel).catch(() => null);
      if (!now) {
        left.push(`${rel}: could not be read`);
        continue;
      }
      const back = rewindText(now.text, f.edits);
      if (back === null) left.push(`${rel}: changed since (or the changed text is there more than once)`);
      else if (back !== now.text) ready.push({ rel, text: back, hash: now.hash });
    }
    if (!ready.length) {
      agent.notice(`Nothing could be rewound here (${left.join("; ")}). Source control can undo it.`);
      return "nothing";
    }
    const message =
      `Undo the changes made from this message on in ${ready.length} file${ready.length === 1 ? "" : "s"}?\n\n` +
      ready.map((r) => r.rel).join("\n") +
      (left.length ? `\n\nLeft as they are:\n${left.join("\n")}` : "");
    if (!(await ask(message, { title: "Rewind the files", okLabel: "Undo the changes" }))) return "cancel";
    const failed: string[] = [];
    for (const r of ready) {
      try {
        await api.files.write(p.id, r.rel, r.text, r.hash);
      } catch (e) {
        failed.push(`${r.rel}: ${errorText(e)}`);
      }
    }
    const done = ready.length - failed.length;
    agent.notice(
      `Rewound ${done} file${done === 1 ? "" : "s"} to before this message.` +
        (failed.length || left.length ? ` Not undone: ${[...failed, ...left].join("; ")}.` : ""),
    );
    return done ? "done" : "nothing";
  }

  // Search in the conversation (24n part 6): Ctrl+F in the pane. Groups and folds with a hit open, step cards with a
  // hit show all; the words are marked with the CSS Highlight API (no change to the messages' own markup).
  let searchOpen = $state(false);
  let query = $state("");
  let hitIndex = $state(0);
  let hitCount = $state(0);
  let searchBox = $state<HTMLInputElement>();
  let paneEl = $state<HTMLElement>();
  let hitRanges: Range[] = [];
  /** The words the view last moved to a hit for. */
  let shownFor = "";
  let hitIds = $derived(searchOpen ? new Set(searchItems(agent.items, query)) : new Set<string>());
  async function openSearch() {
    searchOpen = true;
    const picked = window.getSelection()?.toString().trim() ?? "";
    if (picked && !picked.includes("\n")) query = picked;
    await tick();
    searchBox?.focus();
    searchBox?.select();
  }
  function closeSearch() {
    searchOpen = false;
    query = "";
    clearHighlights();
    agent.input?.focus();
  }
  function clearHighlights() {
    hitRanges = [];
    hitCount = 0;
    if (typeof CSS !== "undefined" && "highlights" in CSS) {
      CSS.highlights.delete("chat-hit");
      CSS.highlights.delete("chat-hit-now");
    }
  }
  function markHits() {
    const q = query.trim();
    if (!searchOpen || !q || !scroller) return clearHighlights();
    const ranges: Range[] = [];
    const walker = document.createTreeWalker(scroller, NodeFilter.SHOW_TEXT);
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      const node = n as Text;
      for (const at of matchesIn(node.data, q)) {
        const r = document.createRange();
        r.setStart(node, at);
        r.setEnd(node, at + q.length);
        ranges.push(r);
      }
    }
    hitRanges = ranges;
    hitCount = ranges.length;
    if (hitIndex >= hitCount) hitIndex = 0;
    if (typeof CSS !== "undefined" && "highlights" in CSS && typeof Highlight !== "undefined")
      CSS.highlights.set("chat-hit", new Highlight(...ranges));
    showHit(false);
  }
  function showHit(scroll = true) {
    const r = hitRanges[hitIndex];
    if (typeof CSS !== "undefined" && "highlights" in CSS && typeof Highlight !== "undefined") {
      if (r) CSS.highlights.set("chat-hit-now", new Highlight(r));
      else CSS.highlights.delete("chat-hit-now");
    }
    if (r && scroll) r.startContainer.parentElement?.scrollIntoView({ block: "center" });
  }
  function nextHit(d: number) {
    if (!hitCount) return;
    hitIndex = (hitIndex + d + hitCount) % hitCount;
    showHit();
  }
  function searchKeys(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      closeSearch();
    } else if (event.key === "Enter") {
      event.preventDefault();
      nextHit(event.shiftKey ? -1 : 1);
    }
  }
  // Rows holding a hit open, so the hit can show.
  $effect(() => {
    if (!hitIds.size) return;
    const extra = rowsToOpen(rows, hitIds);
    untrack(() => {
      if (extra.some((id) => !opened.has(id))) opened = new Set([...opened, ...extra]);
    });
  });
  // Marked again when the words, the conversation or what is open change; the first hit is shown.
  $effect(() => {
    void hitIds;
    void opened;
    void agent.items.length;
    const typed = query;
    tick().then(() => {
      markHits();
      // Only new words move the view to the first hit; lines arriving while searching do not.
      if (typed !== shownFor) {
        shownFor = typed;
        if (typed && hitCount) showHit();
      }
    });
  });
  $effect(() => {
    void query;
    hitIndex = 0;
  });
  // Ctrl+F while the pane has the focus (the editor keeps its own find).
  function windowKeys(event: KeyboardEvent) {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f" && !event.shiftKey && !event.altKey) {
      if (paneEl?.contains(document.activeElement) || paneEl?.matches(":hover")) {
        event.preventDefault();
        void openSearch();
      }
    }
  }

  function openFile(file: string | null) {
    if (!file || !project) return;
    const root = project.path.replace(/\\/g, "/").replace(/\/$/, "").toLowerCase();
    const path = file.replace(/\\/g, "/");
    if (path.toLowerCase().startsWith(root + "/")) editor.open(project.id, path.slice(root.length + 1));
  }

  /** What a "without asking" approval covers, in words: all file changes together, otherwise the tool. */
  function kindOf(tool: string): string {
    if (["Edit", "Write", "MultiEdit", "NotebookEdit"].includes(tool)) return "file changes";
    if (tool === "Bash" || tool === "PowerShell") return "commands (risky ones still ask)";
    return tool;
  }
</script>

<svelte:window onkeydown={windowKeys} />

<section class="agent-pane" aria-label="Agent" bind:this={paneEl}>
  {#if searchOpen}
    <div class="search-bar" role="search">
      <Search size={13} />
      <input
        type="text"
        placeholder="Find in the conversation"
        aria-label="Find in the conversation"
        bind:value={query}
        bind:this={searchBox}
        onkeydown={searchKeys}
      />
      <span class="dim count">{query.trim() ? (hitCount ? `${hitIndex + 1} of ${hitCount}` : "no match") : ""}</span>
      <button type="button" title="Previous (Shift+Enter)" aria-label="Previous match" onclick={() => nextHit(-1)}
        ><ChevronUp size={13} /></button
      >
      <button type="button" title="Next (Enter)" aria-label="Next match" onclick={() => nextHit(1)}
        ><ChevronDown size={13} /></button
      >
      <button type="button" title="Close (Esc)" aria-label="Close the search" onclick={closeSearch}
        ><XIcon size={13} /></button
      >
    </div>
  {/if}
  <div class="conversation" bind:this={scroller}>
    {#if !project}
      <div class="empty">
        <div class="badge" aria-hidden="true"><Bot size={18} /></div>
        <h2>Add a project first</h2>
        <p>Use the + next to Projects in the sidebar to add a folder. Then ask Qanuvi to work on it.</p>
      </div>
    {:else if (agent.provider.id === "claude-code" && agent.info && !agent.info.available) || (agent.provider.id === "qanuvi" && !settings.value.brain.models.length)}
      <div class="empty">
        <div class="badge" aria-hidden="true"><Bot size={18} /></div>
        <h2>Get AI help for free</h2>
        <p>
          Qanuvi's own agent works without a paid plan: with a model on this computer (Qanuvi suggests one that fits) or
          free models from OpenRouter. It takes a few steps.
        </p>
        <button type="button" class="setup" onclick={setUpFreeAi}>Set up free AI</button>
        {#if agent.provider.id === "claude-code"}
          <p class="small">
            Have Claude Code? Install it and make sure the <code>claude</code> command works in a terminal.
          </p>
        {/if}
      </div>
    {:else if agent.items.length === 0}
      <div class="empty">
        <div class="badge" aria-hidden="true"><Bot size={18} /></div>
        <h2>{session ? session.title : "New session"}</h2>
        <p>
          Ask Qanuvi to work on {project.name}. Steps and results show here; changes wait for your approval unless you
          allow them.
        </p>
      </div>
    {:else}
      {#each rows as item (item.id)}{@render row(item)}{/each}
      {#if running}<div class="notice info working">
          <LoaderCircle size={12} class="spin" /> Working…{working ? ` · ${working}` : ""}
        </div>{/if}
    {/if}
    {#each agent.errors.slice(-3) as e, i (i)}<div class="notice error">{e}</div>{/each}
    {#if agent.error}<div class="notice error">{agent.error}</div>{/if}
  </div>

  {#if session && agent.grants.length}
    <div class="grants">
      Allowed without asking: {agent.grants
        .map(
          (g) =>
            `${g.group === "edits" ? "file changes" : g.group}${g.secondsLeft === null ? "" : ` (${Math.ceil(g.secondsLeft / 60)} min)`}`,
        )
        .join(", ")}
      <button type="button" class="link" onclick={() => agent.forgetGrants()}>Ask again</button>
    </div>
  {/if}
  {#if session}
    <div class="usage" title="Tokens sent (including cache writes), tokens read from cache, tokens received, and cost">
      in {formatTokens(session.input_tokens)} · cache {formatTokens(session.cache_read_tokens)} · out {formatTokens(
        session.output_tokens,
      )}
      · {formatCost(session.cost_usd)} · {(session.duration_ms / 1000).toFixed(0)} s{working ? ` · ${working}` : ""}
    </div>
  {/if}

  {#if agent.reopened && agent.reopened.id === agent.selectedId && !agent.handover && settings.value.chatHandover.when === "auto"}
    <div class="reopen-line" role="status">
      Starred chat, {tokensK(agent.reopened.was)} of context · your next message goes on in a fresh session from a
      {agent.reopened.note === null ? "handover note" : "kept note"}
    </div>
  {:else if agent.reopened && agent.reopened.id === agent.selectedId && !agent.handover}
    {@const r = agent.reopened}
    <div class="handover-bar reopen" role="status">
      {#if r.note === null}
        <span
          >Starred chat, {tokensK(r.was)} of context. Your next message starts a fresh session from a handover note (written
          once now) instead of re-reading all of it.</span
        >
        <span class="handover-actions">
          <button
            type="button"
            class="link"
            title="The old session writes its note now; you can read and change it before you send"
            onclick={() => agent.handOver(true)}>Write the note now</button
          >
          <button
            type="button"
            class="link"
            title="The old session goes on and re-reads its whole context at each step"
            onclick={() => agent.resumeAsItWas()}>Resume as it was</button
          >
        </span>
      {:else}
        <span>Your next message starts a fresh session from this note. Change it if anything is wrong or missing.</span>
        <span class="handover-actions">
          <button type="button" class="link" onclick={() => agent.resumeAsItWas()}>Resume as it was</button>
        </span>
        <textarea
          class="reopen-note"
          rows="8"
          aria-label="Handover note"
          value={r.note}
          oninput={(e) => agent.editReopenNote(e.currentTarget.value)}></textarea>
      {/if}
    </div>
  {/if}

  {#if agent.offer && !agent.handover}
    <div class="handover-bar" role="status">
      <span
        >This chat is heavy ({tokensK(agent.context)} of context): each step re-reads it all. Hand over to a new session now?</span
      >
      <span class="handover-actions">
        <button
          type="button"
          class="link"
          title="The agent finishes the edit in hand and writes a note; a new session goes on from it in this chat"
          onclick={() => agent.handOver()}>Hand over now</button
        >
        <button type="button" class="link" onclick={() => agent.notNow()}>Not now</button>
        <button
          type="button"
          class="link"
          title="This chat never offers a handover again (Settings, Agents, Chat handover is for all chats)"
          onclick={() => agent.handoverOffForChat()}>Never in this chat</button
        >
      </span>
    </div>
  {/if}
  {#if waitingMsgs.length}
    <div class="waiting" aria-live="polite">
      <div class="waiting-head">
        <span
          >{agent.handover
            ? "Waiting · goes to the new session once the handover is done"
            : readingNow
              ? "Reading it now · the turn had ended, so it started a turn of its own"
              : `Waiting · the agent reads ${waitingMsgs.length === 1 ? "it" : "them"} after the current step`}</span
        >
        <!-- Only while a message still waits behind the step in hand: stopping a turn that reads it would undo it. -->
        {#if session && session.agent !== "qanuvi" && !agent.handover && waitingMsgs.some((m) => m.waiting === "held" || m.waiting === "sent")}
          <button
            type="button"
            class="link"
            disabled={agent.interrupting}
            title="Ends the command in hand and the agent's turn (like Esc in Claude Code); the session stays and reads your message next"
            onclick={() => agent.interrupt()}
            >{agent.interrupting ? "Stopping…" : "Stop this command, read my message now"}</button
          >
        {/if}
      </div>
      {#each waitingMsgs as m (m.id)}
        <div class="waiting-row">
          <div
            class="waiting-msg"
            title={m.waiting === "held"
              ? m.text
              : `${m.text}

With the agent now: a message it has cannot be taken back.`}
          >
            {m.text}
          </div>
          {#if m.waiting === "held"}
            <span class="waiting-actions">
              <span class="hold">sends in a few seconds</span>
              <button type="button" class="link" onclick={() => agent.sendHeld(m.id)}>Send now</button>
              <button
                type="button"
                class="link"
                title="Drop it: the agent never sees it"
                onclick={() => agent.dropHeld(m.id)}>Don't send</button
              >
            </span>
          {:else if m.waiting === "turn" && session && session.agent !== "qanuvi"}
            <span class="waiting-actions">
              <button
                type="button"
                class="link"
                disabled={agent.interrupting}
                title="The agent already has it: this stops the turn it started for it, and your next message tells the agent to leave it"
                onclick={() => agent.withdraw(m.id)}>Don't send</button
              >
            </span>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
  <form class="composer" class:dropping onsubmit={send} bind:this={composer}>
    {#if mention && project}
      <MentionPicker
        projectId={project.id}
        query={mention.query}
        bind:selected={mentionSelected}
        bind:choices={mentionChoices}
        onpick={pickMention}
      />
    {/if}
    {#if dropping}<div class="drop-hint">Drop to add: images are attached, project files mentioned</div>{/if}
    {#if agent.attachments.length}
      <div class="chips pending">
        {#each agent.attachments as f, i (i)}
          {#if f.kind === "image"}
            <span class="pic">
              <ChatImage image={{ name: f.name, src: `data:${f.mediaType};base64,${f.data}` }} size={64} />
              <button
                type="button"
                class="drop"
                title="Remove {f.name}"
                aria-label="Remove {f.name}"
                onclick={() => (agent.attachments = agent.attachments.filter((_, j) => j !== i))}>×</button
              >
            </span>
          {:else}
            <span class="chip" title={f.name}
              >{f.kind === "page" ? "🌐" : "📄"}
              {f.name}<button
                type="button"
                aria-label="Remove {f.name}"
                onclick={() => (agent.attachments = agent.attachments.filter((_, j) => j !== i))}>×</button
              ></span
            >
          {/if}
        {/each}
      </div>
    {/if}
    <textarea
      bind:value={agent.draft}
      bind:this={agent.input}
      rows="3"
      placeholder={!project
        ? "Add a project first"
        : running
          ? "Add to the task while it works…  (Enter to send; the agent reads it before its next step)"
          : "Ask Qanuvi…  (Enter to send, Shift+Enter for a new line)"}
      aria-label="Message to the agent"
      disabled={!project}
      onkeydown={keydown}
      oninput={input}
      onpaste={paste}
      oncontextmenu={boxMenu}
      onclick={findMention}
      onkeyup={(e) => ["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key) && findMention()}
      onblur={() => (mention = null)}></textarea>
    <div class="tools">
      <AttachMenu disabled={!project} />
      <CommandMenu bind:open={commandsOpen} disabled={!project} />
      <span class="spacer"></span>
      {#if session && !agent.onPlanStep}
        <button
          type="button"
          class="star"
          class:on={session.saved}
          title={session.saved ? "Starred: kept under Saved. Click to unstar" : "Star: keep this chat under Saved"}
          aria-label={session.saved ? "Unstar the chat" : "Star the chat"}
          aria-pressed={!!session.saved}
          onclick={() => session && agent.star(session.id, !session.saved)}
        >
          <Star size={13} fill={session.saved ? "currentColor" : "none"} />
        </button>
      {/if}
      <ModePicker disabled={running && !agent.onPlanStep} />
      {#if running}
        <button type="button" class="stop" title="Stop" aria-label="Stop" onclick={() => agent.stop()}>
          <Square size={12} />
        </button>
      {/if}
      <button
        type="submit"
        class="send"
        title={running ? "Send: joins the task, the agent reads it before its next step" : "Send"}
        aria-label="Send"
        disabled={!agent.draft.trim() || !project}
      >
        <ArrowUp size={15} />
      </button>
    </div>
  </form>
  <div class="agent-line">
    <button
      type="button"
      class="agent-chip"
      title="Choose the model ({agent.provider.name})"
      onpointerdown={(e) => e.stopPropagation()}
      onclick={() => (commandsOpen = !commandsOpen)}
    >
      <span class="badge" style:background={agent.provider.color}>{agent.provider.badge}</span>
      <span>{agent.provider.name}</span>
      <span class="dim"
        >· {modelLabel(agent.provider, agent.model)}{agent.provider.efforts.length ? ` · ${agent.effort}` : ""}</span
      >
      {#if runner.switchingTo && agent.onPlanStep}
        <span
          class="dim"
          title="The agent at work finishes the edit in hand and writes a progress note; the picked model goes on from it"
          >· switching after the edit in hand</span
        >
      {/if}
    </button>
    <button
      type="button"
      class="gear"
      title="Find in the conversation (Ctrl+F)"
      aria-label="Find in the conversation"
      disabled={!agent.items.length}
      onclick={() => (searchOpen ? closeSearch() : openSearch())}
    >
      <Search size={13} />
    </button>
    <button
      type="button"
      class="gear"
      title="Agent settings and configuration files"
      aria-label="Agent settings"
      onclick={openAgentSettings}
    >
      <Settings2 size={13} />
    </button>
  </div>
</section>

{#snippet row(item: Row)}
  {#if item.kind === "group"}
    <div class="tl">
      <button
        type="button"
        class="group {item.state}"
        aria-expanded={opened.has(item.id)}
        onclick={() => toggle(item.id)}
      >
        <span class="chev" class:open={opened.has(item.id)}><ChevronRight size={12} /></span>
        {#if item.state === "running"}<LoaderCircle size={13} class="spin" />{:else if item.state === "failed"}<CircleX
            size={13}
          />{:else}<Search size={13} />{/if}
        <span>{item.label}</span>
      </button>
      {#if opened.has(item.id)}
        <div class="group-steps">
          {#each item.steps as step (step.id)}<StepCard
              {step}
              onopen={openFile}
              onrevert={revertStep}
              reveal={hitIds.has(step.id)}
            />{/each}
        </div>
      {/if}
    </div>
  {:else if item.kind === "fold"}
    {#if opened.has(item.id)}
      {#each item.rows as inner (inner.id)}{@render row(inner)}{/each}
    {:else}
      <button type="button" class="tl fold" onclick={() => toggle(item.id)}
        >{item.count} earlier step{item.count === 1 ? "" : "s"} · show</button
      >
    {/if}
  {:else if item.kind === "user"}
    {#if editing && editing.id === item.id}
      {@const c = changesFrom(item.id)}
      <div class="user editing">
        <!-- svelte-ignore a11y_autofocus -->
        <textarea
          bind:value={editing.text}
          rows={Math.min(10, Math.max(2, editing.text.split("\n").length))}
          aria-label="The message, changed"
          autofocus
          onkeydown={editKeys}></textarea>
        {#if c.steps}
          <label class="check"
            ><input type="checkbox" bind:checked={editing.rewind} /> Also undo the file changes made from here on ({c.steps}
            step{c.steps === 1 ? "" : "s"} in {c.files} file{c.files === 1 ? "" : "s"})</label
          >
        {/if}
        <div class="edit-actions">
          <span class="dim">Goes to a new session (a branch); this one stays as it is.</span>
          <button type="button" onclick={() => (editing = null)}>Cancel</button>
          <button type="button" class="primary" disabled={!editing.text.trim() || running} onclick={resend}
            >Send as a branch</button
          >
        </div>
      </div>
    {:else}
      <div class="user" role="presentation" oncontextmenu={(e) => messageMenu(e, item.text, item.older ? "" : item.id)}>
        {#if session && !running && !item.during && !item.older}
          <span class="msg-actions">
            <button
              type="button"
              title="Edit and send again, as a branch (this session stays as it is)"
              aria-label="Edit and send again"
              onclick={() => startEdit(item.id, item.text)}><Pencil size={11} /></button
            >
            <button
              type="button"
              title="Rewind the files to before this message (undoes the changes made from here on, after a preview)"
              aria-label="Rewind the files to before this message"
              onclick={() => rewindTo(item.id)}><Undo2 size={11} /></button
            >
          </span>
        {/if}
        {#if item.withdrawn}<span
            class="during"
            title="Its turn was stopped; your next message tells the agent to leave it">Withdrawn</span
          >{:else if item.during}<span
            class="during"
            title="Sent while the agent was working; it reads it before its next step">Added while it worked</span
          >{/if}
        {#if item.withdrawn}<s>{item.text}</s>{:else}{item.text}{/if}
        {#if item.images?.length}
          <div class="chips">
            {#each item.images as image, i (i)}<ChatImage {image} />{/each}
          </div>
        {/if}
        {#if otherFiles(item).length}
          <div class="chips">
            {#each otherFiles(item) as name, i (i)}<span class="chip" title={name}>📎 {name}</span>{/each}
          </div>
        {/if}
        {#if item.at}<span class="time">{clockTime(item.at)}</span>{/if}
      </div>
    {/if}
  {:else if item.kind === "text"}
    {#if item.live}
      {#if !looksLikeCall(item.text)}<div class="text live">{item.text}</div>{/if}
    {:else}
      <!-- Escaped first, then formatted (agent/markdown.ts): the text cannot add HTML of its own. -->
      <!-- eslint-disable-next-line svelte/no-at-html-tags -->
      <div class="text md" onclick={openLink} oncontextmenu={(e) => messageMenu(e, item.text)} role="presentation">
        {@html renderMarkdown(item.text)}
      </div>
      {#if item.at}<span class="time agent">{clockTime(item.at)}</span>{/if}
    {/if}
  {:else if item.kind === "tool"}
    <div class="tl" class:fail={item.state === "failed"}>
      <StepCard step={item} onopen={openFile} onrevert={revertStep} reveal={hitIds.has(item.id)} />
    </div>
  {:else if item.kind === "thinking"}
    <details class="thinking tl">
      <summary
        ><Brain size={12} />
        {#if item.live}Thinking…{:else}Thinking{/if}</summary
      >
      <div class="thinking-text">{item.text}</div>
    </details>
  {:else if item.kind === "todos"}
    <div class="todos tl" aria-label="To-do list">
      <div class="todos-head">
        <ListChecks size={13} /> To-do · {item.todos.filter((t) => t.status === "completed").length} of {item.todos
          .length} done
      </div>
      <ul>
        {#each item.todos as t, i (i)}
          <li class={t.status}>
            <span class="mark" aria-hidden="true"
              >{#if t.status === "completed"}<CircleCheck size={13} />{:else if t.status === "in_progress"}<LoaderCircle
                  size={13}
                  class="spin"
                />{:else}<Circle size={13} />{/if}</span
            >{t.text}
          </li>
        {/each}
      </ul>
    </div>
  {:else if item.kind === "approval"}
    <div class="approval {item.state}">
      <div class="approval-head">
        <ShieldQuestion size={15} />
        <span><strong>{item.tool}</strong> · {item.summary}</span>
      </div>
      <pre>{item.detail}</pre>
      {#if item.state === "pending"}
        <div class="approval-actions">
          <button type="button" class="allow" onclick={() => agent.respond(item.id, true)}>Approve</button>
          {#if session?.agent === "qanuvi"}
            <!-- Free and local models: a while at a time. -->
            <button
              type="button"
              title="Also allow {kindOf(item.tool)} without asking for {settings.value.approveMinutes} minutes"
              onclick={() => agent.respond(item.id, true, `minutes:${settings.value.approveMinutes}`)}
              >Allow for {settings.value.approveMinutes} min</button
            >
          {:else}
            <!-- A paid agent: for the session. -->
            <button
              type="button"
              title="Also allow {kindOf(item.tool)} without asking until this session ends"
              onclick={() => agent.respond(item.id, true, "session")}>Allow for this session</button
            >
          {/if}
          <button type="button" class="decline" onclick={() => agent.respond(item.id, false)}>Decline</button>
        </div>
      {:else}
        <div class="decision">
          {item.state === "allowed" ? (item.auto ? "Approved (allowed earlier)" : "Approved") : "Declined"}
        </div>
      {/if}
    </div>
  {:else if item.kind === "result"}
    <div class="result" class:failed={!item.ok} class:stopped={item.stopped}>
      {item.stopped ? "Stopped by you" : item.ok ? "Finished" : "Stopped with an error"} ·{item.seconds.toFixed(1)} s{item.text
        ? ` · ${item.text}`
        : ""}
      {#if !item.ok && agent.provider.id === "qanuvi" && agent.model === "auto" && agent.info?.available && project}
        <button type="button" class="to-auto" onclick={() => project && agent.handToPaid(project.id)}
          >Hand this to {paidAgent()?.name ?? "the paid agent"}</button
        >
      {/if}
      {#if !item.ok && isLimit(item.text) && agent.provider.id !== "qanuvi" && settings.value.brain.models.length && project}
        <!-- The paid agent hit its limit: the free models can go on now. -->
        <button type="button" class="to-auto" onclick={() => project && agent.handToFree(project.id)}
          >Continue with free models</button
        >
      {/if}
      {#if !item.ok && item.text.includes("Switch to Auto") && agent.model !== "auto"}
        <!-- The chosen model is the only one used; when it fails, Qanuvi core offers to choose again. -->
        <button type="button" class="to-auto" onclick={() => (agent.model = "auto")}>Switch to Auto</button>
      {/if}
    </div>
  {:else if item.kind === "notice" && item.note}
    <details class="handover-line">
      <summary>{item.text} · <span class="link">the note</span></summary>
      <pre class="handover-note">{item.note}</pre>
    </details>
  {:else if item.kind === "notice"}
    <div class="notice {item.tone}">{item.text}</div>
  {/if}
{/snippet}

<style>
  .search-bar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
    border-bottom: 1px solid var(--qx-border);
    color: var(--qx-text-secondary);
  }
  .search-bar input {
    flex: 1;
    min-width: 0;
  }
  .search-bar .count {
    font-size: var(--qx-text-small);
    min-width: 56px;
    text-align: right;
  }
  .search-bar button {
    display: inline-flex;
    padding: 2px;
    background: none;
    border: none;
    color: inherit;
    cursor: pointer;
  }
  :global(::highlight(chat-hit)) {
    background-color: color-mix(in srgb, var(--qx-accent) 35%, transparent);
  }
  :global(::highlight(chat-hit-now)) {
    background-color: var(--qx-accent);
    color: var(--qx-bg-panel);
  }
  .user {
    position: relative;
  }
  .msg-actions {
    position: absolute;
    top: 2px;
    right: 4px;
    display: none;
    gap: 2px;
  }
  .user:hover .msg-actions,
  .user:focus-within .msg-actions {
    display: inline-flex;
  }
  .msg-actions button {
    display: inline-flex;
    padding: 3px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-panel);
    color: var(--qx-text-secondary);
    cursor: pointer;
  }
  .msg-actions button:hover {
    color: var(--qx-text);
  }
  .user.editing {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .user.editing textarea {
    width: 100%;
    resize: vertical;
    font: inherit;
  }
  .edit-actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .edit-actions .dim {
    flex: 1;
    font-size: var(--qx-text-small);
  }
  .agent-pane {
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--qx-bg-panel);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-panel);
    /* Visible, so the mode and command menus can open past the pane's edges. */
    overflow: visible;
    position: relative;
    z-index: 2;
  }

  .conversation {
    border-radius: var(--qx-radius-panel) var(--qx-radius-panel) 0 0;
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: var(--qx-space-4);
    display: flex;
    flex-direction: column;
    gap: var(--qx-space-3);
    user-select: text;
  }

  .empty {
    margin: auto;
    max-width: 300px;
    text-align: center;
  }

  .badge {
    width: 36px;
    height: 36px;
    margin: 0 auto var(--qx-space-3);
    display: grid;
    place-items: center;
    border-radius: var(--qx-radius-card);
    background: var(--qx-bg-raised);
    border: 1px solid var(--qx-border);
    color: var(--qx-text-secondary);
  }

  h2 {
    margin: 0 0 var(--qx-space-1);
    font-size: 14px;
    font-weight: 600;
  }

  .empty p {
    margin: 0;
    color: var(--qx-text-secondary);
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-top: 6px;
  }

  .chips.pending {
    align-items: center;
    margin: 0 0 6px;
  }

  .pic {
    position: relative;
    display: inline-block;
  }

  .pic .drop {
    position: absolute;
    top: -6px;
    right: -6px;
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    padding: 0;
    border: 1px solid var(--qx-border);
    border-radius: 50%;
    background: var(--qx-bg-raised);
    color: var(--qx-text);
    font-size: 12px;
    line-height: 1;
    cursor: pointer;
  }

  .chip {
    display: inline-flex;
    gap: 4px;
    align-items: center;
    overflow: hidden;
    max-width: 260px;
    padding: 2px 8px;
    border: 1px solid var(--qx-border);
    border-radius: 10px;
    color: var(--qx-text-secondary);
    font-size: 11px;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .chip button {
    padding: 0 0 0 2px;
    border: none;
    background: none;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }

  .user {
    align-self: flex-end;
    max-width: 85%;
    padding: var(--qx-space-2) var(--qx-space-3);
    border-radius: var(--qx-radius-card);
    background: var(--qx-bg-raised);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .text {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    line-height: 1.55;
  }

  .text.md {
    white-space: normal;
  }

  .text.md :global(p),
  .text.md :global(ul),
  .text.md :global(ol),
  .text.md :global(pre),
  .text.md :global(blockquote) {
    margin: 0 0 6px;
  }

  .text.md :global(ul),
  .text.md :global(ol) {
    padding-left: 20px;
  }

  .text.md :global(h3),
  .text.md :global(h4),
  .text.md :global(h5),
  .text.md :global(h6) {
    margin: 8px 0 4px;
    font-size: 13px;
  }

  .text.md :global(code) {
    padding: 0 4px;
    border-radius: 4px;
    background: var(--qx-bg-raised);
    font-family: var(--qx-font-mono, monospace);
    font-size: 12px;
  }

  .text.md :global(pre) {
    padding: 6px 8px;
    border-radius: 6px;
    background: var(--qx-bg-raised);
    white-space: pre-wrap;
  }

  .text.md :global(pre code) {
    padding: 0;
    background: none;
  }

  .text.md :global(blockquote) {
    padding-left: 8px;
    border-left: 2px solid var(--qx-border);
    color: var(--qx-text-secondary);
  }

  .text.md :global(a) {
    color: var(--qx-accent);
  }

  /* The timeline: steps sit on a thin line, each with a dot. */
  .tl {
    position: relative;
    margin-left: 16px;
  }

  .tl::before {
    content: "";
    position: absolute;
    top: calc(-1 * var(--qx-space-3));
    bottom: calc(-1 * var(--qx-space-3));
    left: -11px;
    width: 1px;
    background: var(--qx-border);
  }

  .tl::after {
    content: "";
    position: absolute;
    top: 10px;
    left: -14px;
    width: 7px;
    height: 7px;
    border: 1px solid var(--qx-text-muted);
    border-radius: 50%;
    background: var(--qx-bg-panel);
  }

  .tl.fail::after {
    border-color: var(--qx-status-failed);
  }

  .group {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 0;
    border: 0;
    background: transparent;
    color: var(--qx-text-secondary);
    font: inherit;
    cursor: pointer;
  }

  .group:hover {
    color: var(--qx-text);
  }

  .group.done :global(svg:not(.chev svg)) {
    color: var(--qx-text-muted);
  }

  .group.failed {
    color: var(--qx-status-failed);
  }

  .chev {
    display: flex;
    color: var(--qx-text-muted);
    transition: transform 0.12s;
  }

  .chev.open {
    transform: rotate(90deg);
  }

  .group-steps {
    display: flex;
    flex-direction: column;
    gap: var(--qx-space-1);
    margin-top: var(--qx-space-1);
  }

  .fold {
    align-self: flex-start;
    padding: 2px 0;
    border: 0;
    background: transparent;
    color: var(--qx-accent);
    font: inherit;
    font-size: var(--qx-text-small);
    cursor: pointer;
  }

  .time {
    display: block;
    margin-top: 2px;
    color: var(--qx-text-muted);
    font-size: 10px;
    text-align: right;
    white-space: nowrap;
  }

  .time.agent {
    margin-top: calc(-1 * var(--qx-space-2));
    text-align: left;
  }

  .composer.dropping {
    border-color: var(--qx-accent);
    border-style: dashed;
  }

  .drop-hint {
    padding: 2px 4px 6px;
    color: var(--qx-accent);
    font-size: var(--qx-text-small);
  }

  .thinking {
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }

  .thinking summary {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
    list-style: none;
  }

  .thinking summary:hover {
    color: var(--qx-text-secondary);
  }

  .thinking-text {
    margin: 4px 0 0 6px;
    padding-left: var(--qx-space-2);
    border-left: 2px solid var(--qx-border);
    font-style: italic;
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .todos {
    padding: var(--qx-space-2) var(--qx-space-3);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
    background: var(--qx-bg-panel);
  }

  .todos-head {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 4px;
    color: var(--qx-text-secondary);
    font-size: var(--qx-text-small);
  }

  .todos ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .todos li {
    display: flex;
    gap: 6px;
    padding: 2px 0;
    line-height: 1.4;
  }

  .todos .mark {
    display: flex;
    flex: none;
    padding-top: 2px;
    color: var(--qx-text-muted);
  }

  .todos li.completed {
    color: var(--qx-text-muted);
    text-decoration: line-through;
  }

  .todos li.completed .mark {
    color: var(--qx-status-done);
  }

  .todos li.in_progress {
    color: var(--qx-text);
    font-weight: 600;
  }

  .todos li.in_progress .mark {
    color: var(--qx-status-running);
  }

  .approval {
    padding: var(--qx-space-3);
    border: 1px solid color-mix(in srgb, var(--qx-status-waiting) 45%, var(--qx-border));
    border-radius: var(--qx-radius-card);
    background: color-mix(in srgb, var(--qx-status-waiting) 7%, var(--qx-bg-panel));
  }

  .approval.allowed,
  .approval.refused {
    border-color: var(--qx-border);
    background: var(--qx-bg-panel);
  }

  .approval-head {
    display: flex;
    align-items: center;
    gap: var(--qx-space-2);
    color: var(--qx-status-waiting);
  }

  .approval-head span {
    color: var(--qx-text);
  }

  .approval pre {
    max-height: 180px;
    margin: var(--qx-space-2) 0;
    padding: var(--qx-space-2);
    overflow: auto;
    border-radius: 6px;
    background: var(--qx-bg-window);
    color: var(--qx-text-secondary);
    font-size: 12px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .approval-actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }

  .approval-actions button {
    height: 24px;
    padding: 0 10px;
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    background: transparent;
    color: var(--qx-text);
    font: inherit;
    font-size: 12px;
    white-space: nowrap;
    cursor: pointer;
  }

  .approval-actions button:hover {
    background: var(--qx-bg-raised);
  }

  .approval-actions .allow {
    border-color: transparent;
    background: var(--qx-accent);
    color: #fff;
  }

  .approval-actions .allow:hover {
    background: var(--qx-accent);
    filter: brightness(1.1);
  }

  /* Declining is the quiet choice: text only, pushed to the end. */
  .approval-actions .decline {
    margin-left: auto;
    border-color: transparent;
    color: var(--qx-text-secondary);
  }

  .decision {
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }

  .result {
    color: var(--qx-status-done);
    font-size: var(--qx-text-small);
  }

  .result.failed {
    color: var(--qx-status-failed);
  }

  .result.stopped {
    color: var(--qx-text-muted);
  }

  .notice {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }

  .notice.error {
    color: var(--qx-status-failed);
    overflow-wrap: anywhere;
  }

  .usage {
    padding: 0 var(--qx-space-3) var(--qx-space-1);
    color: var(--qx-text-muted);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }

  .composer {
    position: relative;
    margin: 0 var(--qx-space-2) var(--qx-space-2);
    padding: var(--qx-space-2) var(--qx-space-2) var(--qx-space-1);
    background: var(--qx-bg-raised);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-input);
  }

  .composer:focus-within {
    border-color: color-mix(in srgb, var(--qx-accent) 55%, var(--qx-border));
  }

  textarea {
    width: 100%;
    padding: var(--qx-space-1);
    border: 0;
    background: transparent;
    color: var(--qx-text);
    font: inherit;
    resize: none;
    outline: none;
    user-select: text;
  }

  .waiting {
    margin: 0 var(--qx-space-2) var(--qx-space-1);
    padding: var(--qx-space-1) var(--qx-space-2);
    border: 1px dashed var(--qx-border);
    border-radius: var(--qx-radius-input);
    font-size: var(--qx-text-small);
  }

  .waiting-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--qx-space-2);
    color: var(--qx-text-muted);
    font-size: 11px;
    font-weight: 600;
  }

  .handover-bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--qx-space-1) var(--qx-space-2);
    margin: 0 var(--qx-space-2) var(--qx-space-1);
    padding: var(--qx-space-1) var(--qx-space-2);
    border: 1px solid color-mix(in srgb, var(--qx-status-waiting) 45%, var(--qx-border));
    border-radius: var(--qx-radius-input);
    background: color-mix(in srgb, var(--qx-status-waiting) 7%, var(--qx-bg-panel));
    font-size: var(--qx-text-small);
  }

  .handover-actions {
    display: flex;
    gap: var(--qx-space-2);
  }

  .reopen-note {
    flex: 1 1 100%;
    min-height: 6em;
    max-height: 40vh;
    resize: vertical;
    padding: var(--qx-space-1) var(--qx-space-2);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-input);
    background: var(--qx-bg-window);
    color: var(--qx-text-secondary);
    font-family: var(--qx-font-mono);
    font-size: var(--qx-text-small);
  }

  .reopen-line {
    margin: 0 var(--qx-space-2) var(--qx-space-1);
    padding-top: var(--qx-space-1);
    border-top: 1px solid var(--qx-border);
    color: var(--qx-text-muted);
    font-size: 11px;
  }

  .handover-line {
    margin: var(--qx-space-2) 0;
    padding-top: var(--qx-space-1);
    border-top: 1px solid var(--qx-border);
    color: var(--qx-text-muted);
    font-size: 11px;
  }

  .handover-line summary {
    cursor: pointer;
  }

  .handover-note {
    max-height: 320px;
    overflow: auto;
    margin: var(--qx-space-1) 0 0;
    padding: var(--qx-space-1) var(--qx-space-2);
    border-radius: var(--qx-radius-input);
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font-family: var(--qx-font-mono, monospace);
    font-size: 11.5px;
    white-space: pre-wrap;
  }

  .waiting-row {
    display: flex;
    align-items: baseline;
    gap: var(--qx-space-2);
  }

  .waiting-actions {
    display: flex;
    flex-shrink: 0;
    align-items: baseline;
    gap: var(--qx-space-2);
    font-size: 11px;
  }

  .waiting-actions .hold {
    color: var(--qx-text-muted);
  }

  .waiting-msg {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--qx-text);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .user .during {
    display: block;
    margin-bottom: 3px;
    color: var(--qx-text-muted);
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.03em;
  }

  textarea::placeholder {
    color: var(--qx-text-muted);
  }

  .tools {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .spacer {
    flex: 1;
  }

  .tools button {
    width: 28px;
    height: 28px;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: var(--qx-radius-button);
    background: transparent;
    color: var(--qx-text-secondary);
    cursor: pointer;
  }

  .tools button:hover:not(:disabled) {
    background: var(--qx-bg-panel);
    color: var(--qx-text);
  }

  .tools .send {
    background: var(--qx-text);
    color: var(--qx-bg-window);
    border-radius: 50%;
  }

  .tools .send:disabled {
    background: var(--qx-border);
    color: var(--qx-text-muted);
    cursor: default;
  }

  .tools .star.on {
    color: var(--qx-status-waiting);
  }

  .tools .stop {
    background: var(--qx-status-failed);
    color: #fff;
    border-radius: 50%;
  }

  .agent-line {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 var(--qx-space-2) var(--qx-space-2);
  }

  .agent-chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 var(--qx-space-2) 0 4px;
    border: 1px solid var(--qx-border);
    border-radius: 999px;
    background: var(--qx-bg-raised);
    color: var(--qx-text-secondary);
    font: inherit;
    font-size: var(--qx-text-small);
    cursor: pointer;
  }

  .agent-chip:hover {
    color: var(--qx-text);
  }

  .agent-chip .badge {
    width: 16px;
    height: 16px;
    margin: 0;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 50%;
    color: #fff;
    font-size: 9px;
    font-weight: 800;
  }

  .agent-chip .dim {
    color: var(--qx-text-muted);
  }

  .gear {
    width: 24px;
    height: 24px;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--qx-text-muted);
    cursor: pointer;
  }

  .gear:hover {
    background: var(--qx-bg-raised);
    color: var(--qx-text);
  }

  :global(.spin) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .setup {
    margin-top: 6px;
    padding: 6px 14px;
    border: 0;
    border-radius: var(--qx-radius-button);
    background: var(--qx-accent);
    color: #fff;
    font: inherit;
    cursor: pointer;
  }

  .small {
    font-size: 11px;
  }

  .grants .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--qx-accent);
    font: inherit;
    cursor: pointer;
  }

  .grants {
    padding: 0 var(--qx-space-3);
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }

  .to-auto {
    margin-left: 8px;
    padding: 0 8px;
    height: 20px;
    border: 1px solid var(--qx-accent);
    border-radius: 6px;
    background: transparent;
    color: var(--qx-accent);
    font: inherit;
    font-size: 11px;
    cursor: pointer;
  }
</style>
