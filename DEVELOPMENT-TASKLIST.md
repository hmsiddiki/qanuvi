# Qanuvi work map

The order of work for Qanuvi. How the parts fit: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). The look:
[docs/DESIGN.md](docs/DESIGN.md) (reference picture in `docs/design/reference-layout.png`).

## Core goals

1. **A great workspace:** light, fast and pleasant enough to replace a heavy editor and browser for daily work, and to
   sit on a second screen while gaming.
2. **Spending fewer tokens, built in beside it:** the right model for each step, small work packets, only what a
   model needs.

**Name:** the app is called **Qanuvi** (folder `E:\Projects\qanuvi`, identifier `dev.qanuvi.desktop`). The owner will provide the logo; until then a "Q" mark is used.

**A code editor first (owner, 2026-11-10):** Qanuvi must work fully as a standard code editor with no AI agent at all,
paid or free, like VS Code. Editing, files, search, terminals, preview, task lists and git never depend on an agent.
AI is a layer the user adds and configures: Settings (Agents) > "AI features" turns the agent pane, sessions and plan
runs on or off; with it off nothing is sent anywhere.

## Rules

- One milestone at a time: build, test, check it in the running app, mark it done, then start the next. The owner
  reviews the app after each milestone.
- Light first: measure memory and CPU of the app at the end of every milestone and record it here. Target for the shell
  with one project open: about 50 to 100 MB without the preview, near 0% CPU when idle. Hidden panes and tabs are
  unloaded.
- Spending fewer tokens is the second core goal: every agent feature must say what it sends to a model and why, and record the
  tokens it used.
- The look follows `docs/DESIGN.md`; changes to the design are made there first.
- Open source from day one: no secrets, keys or personal data in the repository; nothing in the code, docs or commit
  messages names a tool or assistant as an author.
- Every platform from day one: one codebase for Windows, Linux and macOS (desktop) and Android and iOS (handler mode,
  Stage 4). Nothing may be written for one platform only without the others. Each platform's installer is built on
  that platform, so automatic builds (GitHub Actions: Windows, Linux and macOS machines) build all of them on every
  release; development can happen on any of the owner's machines (the Windows PC or the Zorin Linux laptop).

## Status legend

- `[ ]` Not started
- `[-]` In progress
- `[x]` Completed and validated
- `[!]` Blocked; reason underneath

## Stack

- Tauri 2 (Rust core, system web view), Svelte for the interface, CodeMirror 6 for the editor, xterm.js for terminals
  with a real terminal behind it in Rust (portable-pty), SQLite for local data.
- Later: Rust relay service (axum, WebSockets), sign-in through a Qlopy site's OAuth 2.0 + PKCE, Ollama or llama.cpp for
  local models.

## Stage 1: the app shell (first goal)

- [-] **1. Project setup and an empty window** (started 2026-11-10)
  - Owner's Windows PC set up 2026-11-10: Node 24.22, git 2.49, WebView2 157; installed with the owner's permission:
    Visual Studio 2022 Build Tools (C++ 14.44) in `E:\DevTools\VS2022BuildTools` (Windows SDK on C:), Rust 1.99.1 in
    `E:\DevTools\rustup` and `E:\DevTools\cargo`.
  - Done on Windows (2026-11-10): Tauri 2 + SvelteKit (static, single-page) + TypeScript; crate `qanuvi` / `qanuvi_lib`,
    identifier `dev.qanuvi.desktop`; Rust command `app_info` (name, version, OS, architecture) shown in the window;
    design tokens in `src/lib/styles/tokens.css`; window 1440 x 900 (minimum 900 x 560), dark background before the page
    loads, a content security policy instead of none. Tooling: Prettier (with Svelte), svelte-check, Vitest, rustfmt,
    clippy with warnings as errors; `npm run verify` runs all of them plus the Rust tests; `npm run app` (development),
    `npm run app:build` (release). Automatic builds: `.github/workflows/build.yml` (Windows, Ubuntu 22.04, macOS; checks,
    tests, build, installers kept as artifacts). Git repository started (`main`), nothing committed yet.
  - Validated on Windows: `npm run verify` green (2 frontend tests, 2 Rust tests, clippy clean); release build
    `qanuvi.exe` 4.4 MB; the window opens with the dark theme and shows "Version 0.1.0 · Windows x86_64" from the Rust
    core (screenshot checked).
  - Measured on Windows (release build, idle, welcome screen): 7 processes (Qanuvi plus its WebView2), private memory
    76.6 MB in total (Qanuvi 4 MB, WebView2 72.6 MB), CPU 0%.
  - Still open: open and measure on the Zorin laptop (Linux); the macOS build in the automatic builds once the repository
    is on GitHub; Qanuvi's own icon (the default Tauri icon is in place). `npm audit`: 5 low-severity warnings in
    SvelteKit's server-side packages (`cookie`, `deepmerge`), not used by a static app; recheck when SvelteKit 3 is
    adopted.
  - Tauri 2 + Svelte + TypeScript project, Rust workspace, lint and format, a test runner for Rust and for the frontend,
    build scripts that work on Windows, Linux and macOS.
  - Automatic builds (GitHub Actions) for Windows, Linux and macOS on every push, set up once the repository is on
    GitHub.
  - Window opens with the dark theme from `docs/DESIGN.md` (colour, radius and type tokens as CSS variables).
  - Validate: the app builds and opens on Windows (owner's PC) and Linux (owner's Zorin laptop); the macOS build passes
    in the automatic builds; memory and idle CPU recorded on each.

- [x] **1c. A test build keeps its own data (owner, 2026-11-11)**: Qanuvi started from the source tree
  (target/debug or target/release) uses dev.qanuvi.desktop.source for its data and window storage, filled once from a
  copy of the installed copy's data; the title bar says "test build". The installed Qanuvi's data is never touched
  by trying a change (QANUVI_SHARED_DATA=1 turns this off). Live: installed copy and test build open side by side.
  Fixed 2026-11-12: the window storage (WebView2) of a test build is now its own too; before, it opened the installed
  copy's folder, so both shared settings, and a test build with the debug port could not start beside it.
- [x] **1d. Automatic backups and restore (owner, 2026-11-11; done 2026-11-12)**: the owner lost his sessions, memory
  and settings when the uninstaller's "Delete the application data" box was ticked (it deletes for good). The box now
  says "Also delete my Qanuvi data (sessions, memory, settings, model list). This cannot be undone." Done (backups.rs):
  a daily backup (checked 2 minutes after start, then hourly) of the database (VACUUM INTO from a read-only
  connection, checked with quick_check), the other data files (brain.json, agent histories, MCP configs, device file)
  and the window's settings (WebView2 Local Storage). It goes to "Documents\Qanuvi backups" by default, so removing the
  app's data never takes the backups with it (a test build uses "Qanuvi backups (test build)" and never takes the
  installed copy's backups settings). A backup is built under a hidden name and renamed when whole. The last 7 daily
  and manual backups are kept; a before-restore copy stays until removed. Settings → Backups: on/off, folder (a hint
  when it is on the data's drive), last backup or the last error, Back up now, and a list with Restore… and Remove….
  A restore is booked in the pointer file and applied at the next start before anything opens; the data in use is
  backed up first as a "before restore" backup, which can be restored back. A move and a restore cannot both be
  booked. Live on the test build: the daily backup ran on time (20 files, window settings in), and the restore across
  a restart brought it back and kept the data from before. For the owner's review: the Backups page by mouse.
- [x] **15n. Paste and see images in the chat (owner, 2026-11-12)**: Ctrl+V with a screenshot or a copied image did
  nothing; only the + menu attached images, and the chat showed only their names. Now a pasted image (PNG, JPEG, GIF,
  WebP, up to 5 MB) or files copied in the file manager (images, or text up to 200 KB) become attachments; plain text
  pastes as before. Images show as pictures: small ones in the box before sending (x removes), thumbnails in the sent
  message, and the test browser's screenshots; a click shows one whole over the window (Esc or a click closes it).
  Sent images are kept in the data folder's attachments/ (named by their content, so one copy per image) and come
  back when the conversation is opened again. Live on the test build: an image pasted with Ctrl+V showed in the box
  and in the message, Claude Code read it correctly ("orange, QX-42"), and after a restart it showed again and opened
  whole. Not done yet (in 24n): the right-click menu's Paste is text only; dropping files onto the box; removing
  stored images no conversation uses any more.
- [x] **15j. Messages while the agent works (owner, 2026-11-12)**: the box sends while a task runs (Send beside Stop);
  the message joins the task and the agent reads it before its next step (Claude Code takes it mid-turn, Qanuvi's own
  agent adds it to the conversation before each step). It shows "Added while it worked". Live: Claude Code followed a
  change of plan sent during its second step.
- [x] **15l. Plans and files in ignored folders (owner, 2026-11-12)**: a plan in content/plugins/qcommerce was not
  shown because the project's .gitignore leaves out content/plugins/*. The Plan view now looks at every Markdown file,
  ignore rules or not (not node_modules, vendor, build output or backup folders). Search and quick open also take in a
  git repository nested in an ignored folder, with that repository's own rules. Live on amirularham: PLAN-D.md and the
  qcommerce task list show; "amirul" finds 11 matches in 0.3 s.
- [x] **15m. OmniRoute connections: remove, or no warnings (owner, 2026-11-12)**: each connection has Remove (through
  OmniRoute's own command, after a confirmation) and, while it has a problem, Don't warn / Warn again (kept in Qanuvi,
  the connection stays in OmniRoute). Warnings stay on by default.
- [x] **1b2. Updates wait for a click (owner, 2026-11-12)**: a newer version shows as a slim "Update to x" button in
  the status bar (also with AI off); a click downloads it (progress in the same place), then "Restart to update". Not
  restarted, it installs quietly when Qanuvi closes. Checks at start, every hour and when the window comes back after
  15 minutes. Settings → Updates: "Download updates by themselves" (off by default).
- [x] **24h. Plan runs that finish by themselves (owner, 2026-11-12)**: the owner's aim is an assistant that
  completes a whole project unattended, waiting only on risky or complicated steps. Done in 0.1.5:
  - Run dialog: agent picker; one model for every step (cost against the plan's own models, "Write it into the
    plan"); a failing step moves to a stronger model (on/off); who works (Team settings, paid only, free first); the
    settings kept per plan; Fable 5.2 replaces Fable 5.1.
  - Messages during a step are guidance: followed now, "do not" overrides the step, a small extra is done and noted, a
    big one becomes a new step (82a-1) that runs next, "go back to 82b" reopens it and the run does it next, a plain
    "stop" stops the run. They no longer take over the run's events or change its mode (the 82a/82b stalls).
  - Run notes: what the owner says and what steps leave ("Note for the next steps:") reach every later step.
  - Project secrets (Project settings): in the credential store, given to agents and the test command as
    environment variables; a password typed into a running step is offered as a secret instead.
  - Steps in a row on one model and section continue one session (up to 80k tokens of context and 6 steps; each step
    counts its own cost); a new session gets the notes.
  - A blocked step that most remaining steps need stops the run and says what is missing, instead of skipping them.
  - Pickers on a running plan step show and change it now (mode for the rest of the run, model into the step's line).
  - Stop always works; the running step's dot turns; "This run: n of m · Plan: a of b done"; marking a step done by
    hand asks first.
  Live (scratch plan, Haiku): a mid-step message was followed and kept as a note, step 2 continued step 1's session,
  the mode stayed Auto, the run finished both steps; a project secret reached the test command.
- [x] **24i. Before a run (owner, 2026-11-12)**: "Check what the run needs from me first" (on by default): the agent
  reads the open steps once and lists logins (saved as project secrets), decisions (kept as plan notes), risky steps
  (wait or go ahead) and things to set up; open questions and risky steps to wait at pause the run there. Live: the
  scratch plan's missing key, open choice and risky delete were found; the run did step 6 and stopped before step 7.
- [x] **24l. Runs go on through usage limits; Team per run (owner, 2026-11-12)**: a usage limit ("resets 5:20am") is
  no failure: the run waits for the reset and tries the step again, or lets free models work until then (Run dialog,
  "When the paid agent hits its usage limit"). The Run dialog has "Team for this run" with the Settings modes; Team has
  a new mode "Free decides, free works" (runs and the run agent go to Qanuvi's own agent with the free models). A chat
  that hits the paid limit offers "Continue with free models". Plan steps get logs, notes and private details across
  sessions; a stopped step's session resumes the plan.
- [x] **1g. Claude Code kept current (owner, 2026-11-12)**: Settings, Agents shows Claude Code's version against the
  newest (npm registry) with "Update Claude Code" (its own `claude update`) and "Keep it up to date by itself".
  Qanuvi does not bundle Claude Code (Anthropic's software). Release rule in scripts/publish-update.mjs: no release
  while Claude Code is out of date or knows a newer model (per family) than Qanuvi offers.
- [x] **24m. Plans that run unattended and cheaply (owner, 2026-11-12; 0.1.10 and 0.1.11)**:
  - The plan's shared knowledge per plan file in .qanuvi/plan-logs/<plan>/ (kept out of git): MAP.md (what is where,
    kept by the steps), FACTS.md (decisions, conventions, how to run and test, gotchas; owner's and steps' notes go
    there), API.md (what the plan added, written by Qanuvi from the code index, no model tokens), a log per step
    (attempts, failures, results, files changed). Every step gets MAP and FACTS first (cache-friendly) and keeps them
    small (80 / 60 lines). Plans that need or name other plans are told where those keep their knowledge.
  - Steps use the code index (hint in every step), get the files the last steps changed (by modification time), and
    code hints only from the plan's own folder (no vendor code). Sessions go on across sections (same model, under
    150k tokens). Full test suite once per section by default.
  - Run control: one header button (Run / Resume / Pause + Stop). Pause and Stop wait only for the edit in hand; Pause
    writes a progress note in the step's log and keeps the session; Resume opens the run's settings as they were and
    goes on in the same session, also after a restart (the paused session is saved with the run; if it cannot be
    reopened, a new session reads the progress note).
  - Cost: "per step at most $X / N minutes" pauses a runaway step; the run panel shows cost per finished step against
    the plan's earlier runs, the plan's spend so far, and tool calls by kind (reading / changing / running).
  - Live tests on the scratch plan only (see feedback memory: never on the owner's folders).
- [ ] **CHECKPOINT 2026-11-12 (resume here in a new conversation)**:
  - Published: 0.1.11 on the dev channel (https://qlopy.top/qanuvi-updates/dev/latest.json). Published with
    QANUVI_RELEASE_ANYWAY=1 because Claude Code on this PC is 2.1.302 (2.1.322 is out): the owner runs `claude update`
    (or Settings, Agents, Update Claude Code) so the release rule passes next time. Never publish without the owner's yes.
  - Measured: in a 30-minute window, Qanuvi's PLAN-D step 83b sent about 3.3M context tokens ($2.16) while the long
    development conversation sent about 21.6M (contexts up to 732k tokens). The development conversation, not Qanuvi,
    was eating the 5-hour window; keep development conversations short.
  - Owner's PLAN-D (content/plugins/qcommerce/PLAN-D.md in the amirularham folder): 82a-82z and 83a done, 83b was
    running on Opus 5.5 under 0.1.10. Advice given: update to 0.1.11, pause (it now waits only for the edit), put the
    QCOMMERCE_TEST_* login in Project settings, Secrets, run on Opus 5.5 or Fable 5.2 with "Check what the run needs"
    on and a per-step limit.
  - Linux CI: the Ubuntu job twice ended with "the hosted runner lost communication" in the Rust tests (Windows and
    macOS pass). Found 2026-11-12 (scripts/ci-linux-tests.sh runs each test alone in a cgroup with limits):
    checks::tests::stops_a_command_that_runs_too_long killed its own test process; the timeout ran the `kill`
    program as `kill -9 -<pid>`, which some versions read wrongly and kill every process of the user (the runner
    too). Fixed: checks::kill_group calls killpg directly, only for a real group leader (also used by agent_tools).
    Then the test browser took a read interrupted by a signal (EINTR) for a closed connection; now retried. DONE:
    CI green on all three platforms (run 43146222756, commit 54cf0f5). Both fixes are unpublished, like the winget one.
  - Committed after 0.1.11, not published yet: the Update Claude Code button also works for winget installs (runs
    winget upgrade). Publish it with the next batch, after asking the owner.
  - Done 2026-11-12 after this checkpoint, committed and pushed, NOT published (ask the owner first): 1d backups,
    15n paste and see images in the chat. Unpublished since 0.1.11 in all: those two, the winget update fix and the two
    Linux fixes.
  - Done 2026-11-12 (later), committed, NOT published: 24n parts 1 and 4 in part: steps as cards (StepCard.svelte)
    with IN and OUT in mono blocks (fold after 6 lines, Show all, copy, Open the file; shell steps open by default,
    a screenshot opens its card), thinking shown folded, TodoWrite as one live checklist card. Still open in that
    step: part 2 (timeline dots, times on messages, "Read 5 files" grouping, long runs folded) and live thinking as it
    streams. Not yet looked at in the running app: check the cards on a scratch project first.
  - Done 2026-11-12 (owner asked: "avoid heavy cost, but code quality first"), committed, NOT published: 24p handover
    inside a step. Between steps a new session already started past carryLimit (150k) or 6 steps; inside one step
    nothing watched the context. Now each streamed answer's context size is read (no extra reads); past the run's
    "Hand a long step over" limit (Run panel, default 200k, 0 = off) the agent finishes the edit in hand, writes a
    Progress note in the step's log (done and checked, left, files and why, decisions, facts learned), stops; a new
    session checks the earlier work first and goes on. A handover is no failed try (outcome "stopped"); at most 4 per
    step; Qanuvi's fix rounds skip the old session; a step marked done goes through the usual tests and review. Code:
    plan/guidance.ts (handoverDue, handoverRequest, HANDOVER_RESUME), stores/runner.svelte.ts. To see live: a long step
    on the scratch plan, then compare cost per step with earlier runs.
  - Done 2026-11-12 (owner: "do 1 2 3 then publish"), verify green (219 + 152 tests):
    (1) 24n part 2: timeline (dots on a thin line; times on messages, kept in session_events.at, migration 4; old
    lines have none), 3+ look-around steps in a row as one "Read 5 files · searched 2 times" row, runs of more than 10
    steps folded keeping the last 4, thinking shown live while written. (2) 24n part 5: @ mentions of files and
    symbols (MentionPicker.svelte; ↑ ↓, Enter/Tab, Esc), files dropped onto the box (images attached, project files
    mentioned, others attached; Tauri drag-drop event), right-click menu with Paste that takes images. (3) 24n part 3:
    Edit/MultiEdit/Write steps show a diff (+N −M, folded past 14 lines) with Open and Revert (only when the edited
    text is still in the file once; asks first; a whole-file Write has no Revert). Not driven in the running app yet:
    the owner checks after the update. Next: (4) 24j plan housekeeping.
  - PUBLISHED 0.1.12 on the dev channel 2026-11-12 (owner: "release anyway"; QANUVI_RELEASE_ANYWAY=1, Claude Code
    here 2.1.318, 2.1.322 out): everything listed above since 0.1.11. Nothing unpublished now. Next: (4) 24j.
  - Done 2026-11-12 (later), committed, NOT published (ask the owner first): (4) 24j plan housekeeping (archive and
    add steps, see 24j). Verify green (231 + 152 tests). Next: (5) 24o finishing pass.
  - Fixed 2026-11-12 (owner's report: set 93a to Fable, the run used Sonnet), committed, NOT published: the Plan view
    kept the file it read last; after an agent added steps, a model change was refused (the file had changed), the
    reload wiped the error at once, and the dropdown still showed Fable. Now every change in the Plan view (model,
    status, title) reads the file as it is, finds the step by number and title, and writes; a failure stays on screen
    and the dropdown goes back to what the file says; the view re-reads the file every 4 s while open.
  - Done 2026-11-12 (owner: a model picked while a step works should take over after the current job, with the
    context), committed, NOT published: picking a model in the chat footer, or in the running step's dropdown in
    the Plan view, now hands the step over: the agent at work finishes the edit in hand, writes a Progress note in
    the step's log and stops; the chosen model goes on in a new session that checks the note first (no failed try,
    not counted against the handover limit; stopped after 3 minutes if it does not stop). Before, the pick only
    applied from the next try or fix round. Each step now starts on its Model line as it is when it starts, so a
    later step's model changed during a run is used (before, the models read at the run's start were used). Code:
    guidance.ts (modelSwitchRequest, SWITCH_RESUME), runner switchStepModel. Not yet driven live.
  - PUBLISHED 0.1.13 on the dev channel 2026-11-12 (owner: "publish anyway"; QANUVI_RELEASE_ANYWAY=1): 24j plan
    housekeeping, the Plan view stale-file fix, and the model switch with handover. Nothing unpublished now.
  - Done 2026-11-12 (later), committed, NOT published (ask the owner first): (5) 24o finishing pass, with the two
    open notes below folded in (archive numbers, footer switching text). Verify green (248 + 152 tests). Next: (6).
  - Done 2026-11-12 (later), committed, NOT published (ask the owner first): (6) 24n part 6, edit and resend, rewind
    and search (see 24n). Verify green (257 + 152 tests). Unpublished since 0.1.13 in all: 24o and 24n part 6.
  - Done 2026-11-13, committed, NOT published: 15i part 1 code (language servers; see 15i "Built"). Verify green
    (262 + 156 tests). Not yet seen with a real server: no server is installed on this PC yet (rust-analyzer's
    rustup component is missing too); installing one downloads from npm or rustup, so it waits for the owner's yes.
  - Done 2026-11-13, committed, NOT published: 15i part 1 live check on a scratch project with the release build
    (see 15i "Live check"); five fixes from it. Verify green (265 + 156 tests).
  - Done 2026-11-13 (later), committed, NOT published: 15i part 2 code (formatting, quick fixes, "Fix with agent";
    see 15i "Built (part 2)"). Verify green (273 + 156 tests; no Rust change). Checked against Intelephense and
    rust-analyzer with a probe script; not yet driven in the window.
  - Done 2026-11-13, committed, NOT published: 24r code (see 24r "Built"). Verify green (278 + 156 tests). Not yet
    driven in the window, and Claude Code's exact timing of reading a queued message was not probed (a headless probe
    was blocked on this PC); check it on a scratch project first.
  - Owner agreed 2026-11-13: 24s in the lean form (below), next after the 24r check.
  - Done 2026-11-13 (later), committed, NOT published: 24s part 1 code (see 24s "Built (part 1)"). The owner drives
    24r on a release build of 46c2cb2 (copied out of target/release) with a scratch project (slow.sh, PLAN-CHECK.md).
  - Done 2026-11-13 (later): 24r and 24s part 1 driven live on a scratch project (see their "Live check"); fixes
    committed, NOT published.
  - Done 2026-11-13 (later), committed, NOT published: 24s part 2 code, bookmarks (see 24s "Built (part 2)").
    Verify green (293 + 156 tests). Not yet driven in the window.
  - PUBLISHED 0.1.14 on the dev channel 2026-11-13 (owner: "publish anyway"; QANUVI_RELEASE_ANYWAY=1): 24o, 24n
    part 6, 15i parts 1 and 2, 24r, 24s parts 1 and 2. Nothing unpublished now. Owner agreed 24t chat clean-up.
  - Done 2026-11-13 (later), committed, NOT published: 24t chat clean-up (see 24t "Built"). Verify green (293 + 156).
  - Done 2026-11-13 (later), committed, NOT published: 24s part 3 (see 24s "Built (part 3)"), then tied to Chat
    handover (Automatic: no bar, no click; Ask me first: bar and note panel; Off: resumes). Verify green (295 + 156).
  - UNPUBLISHED since 0.1.14 (commits ff286e5, b6700ae, 9800403): 24t chat clean-up, 24s part 3. None driven live
    yet. Publish only with the owner's yes.
  - Done 2026-11-13 (later), committed, NOT published (owner asked, from a real chat that seemed stuck): (a) a message
    sent while the agent worked that Claude Code took in only after its turn ended started a turn of its own, but the
    chat showed "Finished" until the first answer (70 s of thinking on a 126k chat); now the turn's init line sets the
    session running, and the waiting area says "Reading it now" (state "turn") and hides "Stop this command" (it would
    stop the very turn reading it). (b) Messages sent while it works are kept in Qanuvi 10 s first (agent/waiting.ts
    HOLD_MS): Send now / Don't send; they go at once when the turn ends, another chat opens or "read my message now"
    is pressed; messages with files are not kept back. In state "turn", Don't send stops that turn and the next message
    tells the agent to leave it (withdrawnNote, hidden from the shown text); the row shows "Withdrawn". (c) Plan view:
    Reload and opening the plan list look for new plan files (a PLAN-E written by an agent did not show until the
    project was reopened). Verify green (297 + 156). Not driven live yet.
  - Owner question 2026-11-13: steps with no Model: line are routed at run time (routing.ts chooseModel + Team
    decider); offered to keep step model picks out of the plan file (model ids hold a word the owner's no-attribution
    check searches for). Not agreed yet.
  - PUBLISHED 0.1.15 on the dev channel 2026-11-13 (owner: "publish anyway"; QANUVI_RELEASE_ANYWAY=1): 24t, 24s
    part 3, the waiting-message hold, the Plan view's search for new plan files. Owner declined keeping step model
    picks out of the plan file.
  - Done 2026-11-13 (later), committed, NOT published (owner: "it's important"; done the way VS Code's chat does it):
    a step card offers Revert only when no later step changed that file; otherwise "Rewind to here" (this change and
    every one after it, all files, after the preview), so a file is never left half old, half new (sessionTools
    laterChanges). After Revert the card shows Redo; after a rewind a bar above the box offers Redo. Redo writes back
    exactly what the undo replaced, and only while the file is still as the undo left it. Undo records live in the
    window for the open chat only. Verify green (298 + 156). Not driven live yet.
  - Done 2026-11-13 (later), committed, NOT published (owner agreed A + B; "quality first, saving tokens second";
    "only if the user wants"): "Pick models" in the Plan view (plan/picks.ts, runner.pickModels, decider pickPrompt /
    parsePicks): the paid agent reads the open Auto steps once and picks a model each, quality first, seeing per kind
    of work each model's steps done of tried, tokens and cost per finished step; a model burning 1.5x the tokens of
    the lightest proven one (3+ tries, 80%+ done) is marked HEAVY and picked only when needed (routing modelProfiles;
    tokens only, not price). Picks are kept on this computer, not in the plan file; a step's own Model: line (the
    owner's dropdown choice) is never replaced. Runs never pick models by themselves: they use saved picks (same agent,
    same step title) and otherwise run as before (the free-first worker call unchanged). Order at a step: own Model:
    line, project rule, the pick, learned, default. The dropdown shows "Auto · <model>" with the reason. Owner's real
    record (2026-11-13, security work): Fable 5.2 2/5 done at about 337k tokens per finished step (HEAVY), Opus 5.5
    33/34 at 220k, Sonnet 5.5 5/5 at 198k. Verify green (305 + 156). Not driven live yet.
  - Owner answered ("of course the most reliable"): with no pick, the learned rule now takes the most reliable proven
    model (routing reliability: Wilson lower bound, so 33/34 beats 5/5), cost only as a tie-break. Verify green
    (305 + 156). Published as 0.1.16 with Revert/Redo and Pick models (owner: "publish anyway").
  - Driven live 2026-11-13 on a scratch project (test build): Pick models (3 steps picked, run used them, $0.01), Revert and Redo, Rewind to here (preview lists files left alone) and its Redo (files back byte for byte), bookmarks (star moves the chat under Saved), the waiting-message hold (Send now / Don't send, held message read after the turn), the heavy-chat bar at 37k. Fixed: picked steps say "Picked · <model>", not "Auto"; picks of steps without a number follow their title when a run moves lines; the result line "Finished · 4.9 s" had no space and read as a minus. Then fixed (driven live): steps written "1a. ..." without bold are read as numbered (one log per step, renames keep the number); quiet asks (the check before the run, chat review) now pass their session status on, so the status bar no longer keeps "1 working" after they answered. Published as 0.1.18. Published as 0.1.17 (owner: "release anyway"). Still to drive: 24t clean-up, 24s part 3 reopen, 15i part 2.
  - 24u part 1 driven live 2026-11-13 on a scratch project (1a asks for a choice left empty in the check, 1b same
    number, 1z, 2a; Haiku 5.5, $0.04 in all): 2a and 1z ran, 1b set aside "waits for 1a", the run waited ("Waiting
    for you (1 answer)", Needs your answer card with reply box, Try again, Skip this step); answering "dash" + Try
    again ran 1a with the answer, released 1b, run finished 4 of 4; a second run ended by Finish without them (summary
    "2 of 4 done, 2 set aside", 1a [!], 1b left open). Findings for the owner, not changed yet: (a) a question left
    open on the run's FIRST step does not hold it (runner `!firstStep` rule), so 1a ran and blocked itself ($0.004)
    instead of waiting unrun; (b) the z step ran before the released steps and is not run again after them, so the
    README missed slugify (the 1b agent noted it as still open).
  - Both fixed (owner: "yes", 2026-11-13), verify green (313 + 156), driven live on the same scratch plan ($0.12):
    (a) a hold from the check applies to the run's first step too (the `firstStep` exemption is gone): 1a waited
    unrun with Go ahead; (b) a step that finishes after its section's closing step ran opens that closing step again
    (runner `#reopenClosing`, StepRun `rerunAfter`, note `closingRerunNote`): 1z ran again after 1a and after 1b and
    the README now names slugify. The re-run note now also says Qanuvi opened the step on purpose (the live 1z agent
    wondered who changed its [x]); that wording change is not driven live.
  - 24u part 2 built and driven live 2026-11-14, with the closing-step catch-up across runs (see Part 2 below).
    RESUME HERE (new conversation, checkpoint 2026-11-14, session B done): 24u part 4 session B BUILT AND CHECKED
    LIVE on Windows (07d8e38, unpublished, verify green 340 + 166). New Inbox items show as desktop notifications
    (core decides in src-tauri/src/notify.rs: which items, quiet hours, items within 3 s told as one; showing and
    clicks in desktop_notify.rs); Settings > Notifications (on/off, questions / + problems / everything, several as
    one, quiet hours, test button); a click opens the Inbox at the item (ui.inboxFocus, marked 4 s). Windows: an
    installed Qanuvi sends under its own app id (shortcut AUMID dev.qanuvi.desktop, checked) with a protocol launch
    qanuvi://inbox/<id>; the NSIS installer registers qanuvi:// (checked in HKCU\Software\Classes\qanuvi); single
    instance (installed builds only, test builds run beside it) forwards the link; restarts (Restart, Restart to
    update) go through app_restart, which waits for the old instance, because a plain relaunch would hand over to the
    old one and quit. Test builds send under PowerShell's app id and read the banner click in the process. Driven
    live 2026-11-14 with the build installed over D:\qanuvi (owner's yes): qanuvi://inbox while running = one
    process, window to front, Inbox open; owner clicked the test notification on the banner, in the notification
    centre after the time-out, and with Qanuvi closed: all three opened the Inbox. Driven live 2026-11-14 (test
    build, scratch project, PLAN-K, $0): the check held 1a ("needs you"), the run waited, a banner came for the
    question, the owner clicked it and the Inbox opened with that question's item marked; Skip from the run panel
    closed the question ("You answered: Skip this step") and a "finished" item came in. NOT driven live: bundling,
    quiet hours, the restart change, an item link through the installed build's protocol launch. Linux: notification code cross-builds with zigbuild in a throwaway crate; click not
    tested (Zorin laptop). macOS: code review only. The Linux server build (--no-default-features) fails in
    secrets.rs (keyring), 8 errors, already before session B (keyring is only in the desktop feature, but agent.rs
    calls secrets::env in the master too); proposed to the owner, not built yet: without desktop, env/names return
    empty and saving says "this build has no credential store"; an encrypted file store on servers later if wanted.
    FIX 2026-11-14 (owner's PLAN-E run, unpublished, verify green 340 + 166): the finishing pass panel kept "at
    work" and "1x $0" after the pass ended (FINISH.md said done). Cause: finishPass changed its local object after
    putting it on the deep-state run, which holds its own copy (checked with a Svelte probe), so the panel and the
    saved run never saw the end. Now it changes run.finish; a restored run with a pass still "running" shows it
    stopped with a note. Done-line rule now says: edit with the file edit tool, never a shell/regex replace (96b's
    note lost "$45"/"$0" amounts that way). Not driven live yet: watch the next finishing pass end in the panel.
    24s PART 3 DRIVEN LIVE 2026-11-14 (test build, scratch project, Sonnet 5.5, $0.43 in all): a chat read
    prices.js (115 KB) to 76k, starred, Qanuvi closed and started again: the chat reopened with the bar "Starred chat,
    76k of context ... Write the note now / Resume as it was", no tokens spent by opening. Write the note now: the old
    session wrote it ($0.02), the editable panel showed it; a line added by hand ("blue-kiwi") reached the new session,
    which answered the test word and the count at 12k ($0.06). One linked session in the DB, the star moved to it.
    Findings for the owner, not changed yet: (a) the note went as a turn of its own (chatResumePrompt with no request:
    "check what the note calls done, go on with what is left"), so the new session re-checked the count with Grep and
    said "nothing left" before the owner's message ran as a second turn; proposed: on reopen the owner's message joins
    the first turn ("the user's new message says what comes next; check only what it needs") and shows as their
    bubble. (b) the old session stays alive (idle, about 200 MB claude.exe) after the note is written and the new
    session started; proposed: stop it once the new session has started. Not driven: Resume as it was, Automatic.
    BOTH FIXED (owner: "ok fix", verify green 341 + 166): on reopen the owner's message joins the first turn with
    the note (chatResumePrompt `next`, parseResume reads it back, the chat shows it as the user's bubble under "New
    session"); messages sent while the note is written join it too (#goOnFresh joinFirst); the old session is stopped
    once the new one started (also after a plain handover). Driven live (two new 70k+ starred chats, restart, just
    sending, $0.10 for both new sessions): one turn each, answered straight from the note with no file read, 11k;
    old sessions "stopped" in the DB, only the two new agent processes left. The "write the note first, then send"
    order was not driven again after the fix (same #goOnFresh path).
    UPDATE 2026-11-14 (newest): 15i PART 4 BUILT AND DRIVEN LIVE (split editor + minimap, unpublished, verify
    375 + 172), plus the owner's NEW LOGO everywhere (54fa123: purple tile with a cut square; icons, title-bar logo,
    favicon; v11 kept in docs/design). See 15i "Built (part 4)" and "Driven live (part 4)". Both findings FIXED (owner:
    "do as you suggested") and driven live. With part 4, 15i's order is done (Phpactor stays LATER). Owner: do NOT
    publish 0.1.20 yet.
    FIXED (owner, 2026-11-16 09:00): FALSE USAGE LIMIT stopped the owner's TINYCODE-FINAL-PLAN run on 0.1.22 for an
    hour after step 13 (08:18). The limit check ran on whole raw agent lines (last 8): a tool result quoting the plan
    said "REST rate limit" and later "reached", so `rate[_ ]limit.*reached` matched, no reset time -> wait an hour,
    session stopped, nothing saved, run looked stuck. Fix in the Rust runner (plan/limits.rs limit_notice, run.rs
    limit_reset): only the agent's own error end counts (result with is_error, an assistant API-error message,
    non-JSON agent output), checked on its text, at most 400 characters; tool results, ordinary replies, successful
    ends and owner messages never do. The fake env now keeps every agent line with its session as the core does (the
    old usage-limit scenario only passed through the runner's own echo). Tests: limit_notice cases, scenario "a step
    about a rate limit is not the agent's usage limit"; 427 Rust lib tests. The installed 0.1.22 window runner keeps
    the old check until the next published update (owner's yes).
    Finding b1 (owner yes 2026-11-17, to build after M3): a run stopped by its budget looked "blocked". QVTHEME-PLAN
    run had a $1.00 budget, step 1 on Opus cost $1.16, the run stopped after it; the reason was only in the Inbox
    ("reached its budget of $1.00"), the plan panel showed a done step and Resume. Fix: (a) before a run starts, warn
    when its budget is below the expected cost of the first step (its model's usual step cost); (b) the plan header
    says why a run stopped ("Stopped: budget $1.00 reached") with a Raise budget action next to Resume.
    PICKUP POINT (fresh conversation starts here, 2026-11-18 13:30): M7c BUILT + DRIVEN LIVE on a Windows scratch
    data folder AND on the VPS basevm378 (throwaway --data, removed after; serve runs the M7c server build, M7b one kept as .prev; no agent there, so the run showed the "not found" path): `qanuvi-master plan run <plan file> [--steps 2-6] [--budget <dollars>]
    [--no-follow]`, `plan follow [<project>]`, `plan list`, `plan stop [<project>] [--now]`. The file picks its
    project (the deepest one holding it). The run copies the setup of the last run a window started on this master
    (run-starts/<project id>.json, else the newest one of another project with its model rules left out; none:
    "start one plan run from a Qanuvi window first"). Following prints step lines (started on model, done with cost,
    failed/blocked with the note, waits for your answer, set aside, skipped), new notices, usage-limit waits, Inbox
    questions, the agent's text and tools (paths from the project root) and the end line; Ctrl+C only leaves
    (the run goes on); stop is gentle, --now stops at once (Controls::stop twice). Code in
    src-tauri/src/master_cli_plan.rs, 8 tests. Drive (scratchpad m7c.sh, m7c-start.js writes a Haiku kept start):
    refusals, a 2-step Haiku run followed, follower killed mid-run (run went on), `plan follow` to the end, --steps 1,
    busy refusal, gentle stop shown as "Stopped by you". Findings fixed in the same build: x1 tool paths from the
    project root, x2 a done try no longer says "it goes on", x3 a busy project is refused before the setup line.
    CI NOTE (owner: fix later): CI fails since M4c 9b15c6d: Linux link needs libxdo-dev (tray-icon via muda) in
    build.yml apt line; macOS a Rust test fails since M5a 9e242f3 (name only in the signed-in log); Windows flaky
    checks::kill_tree test (20 s wait for PowerShell, give 60 s). y1 fixed (a note's own full stop before "; it tries again"). y2 FOR THE OWNER (runner, not M7c): on the VPS without
    Claude Code, step 1 failed and waited for an answer, then started again by itself and asked a second time, and
    plan list counted 2 steps for a 1-step plan. y2 CHECKED (owner, 2026-11-18): (1) FIXED: a run whose agent is not on the machine is refused at its start
    (agent::missing, RunEnv::agent_missing; master plan_run_start/finish say it at once), no model ladder; scenario
    test y2_an_agent_…; driven with PATH without claude (refused, nothing tried) and with it (run done). (2) FIXED (owner yes): a step that failed on every model ran a second time in the same run (guidance::reopened_steps
    counts Failed as passed, but a failed step leaves no mark in the plan, so it always looks sent back); the retry
    setting (tries per model, window default 2) is not the cause and still climbs models inside the step. Now only
    blocked and skipped steps come back (both leave a mark in the plan); a failed one comes back through its Inbox
    answer. Scenario test y2_a_step_that_failed_on_every_model_runs_once_and_asks_once (fails without the fix); driven
    live (Haiku-only, FAIL.md: one copy, one question, step 2 done). Rust 505. Next: the CI fixes, then M8.
    CI FIXED (2026-11-18, ff5f011 + 5dced09): all three green for the first time since M4c. Causes: Linux link needed
    libxdo-dev (build.yml); macOS was never a test but a clippy dead-code error (master_notify APP_ID used only on
    Windows), hidden because a carriage return cut the annotation; Windows failed a master run test since y2 part 1
    (runners have no Claude Code; the test now starts with Qanuvi agent) and the kill_tree start wait is 60 s.
    ci-verify.sh now strips colour codes and carriage returns and posts failing Rust tests by name in their own
    annotation. The owner's Linux laptop (Zorin OS 18.1, on the same wifi, reached over SSH) is set up as a Linux check
    machine: Rust + Node 24 in the user's home, clone in ~/src/qanuvi, ~/src/verify.sh runs CI's verify
    (Zorin needs libayatana-appindicator3-dev, not libappindicator3-dev). M8 PARTS AGREED (owner, 2026-11-18):
    M8a check on the master, M8b `qanuvi-master update`, M8c the screen for a shown master, M8d This PC's whole
    install (docs/MIGRATION-VIEWER.md M8). PICKUP POINT (2026-11-18): next build M8a; the laptop can run
    ~/src/verify.sh (git pull first) for a Linux check before pushing.
    M8a BUILT + DRIVEN LIVE (2026-11-18, Windows scratch masters + the Linux laptop; VPS drive waits for a yes): every
    master looks on the update channel itself (master_update.rs: two minutes after start, then hourly;
    QANUVI_UPDATES_URL for a test channel, QANUVI_NO_UPDATE_CHECK=1 turns it off) and answers update_status and
    update_check. Build keys: This PC's master reads the app installer's (windows-x86_64; it installs the whole app,
    M8d), every other master master-<os>-<arch>. verify() checks a build's minisign signature against the app's public
    key (minisign-verify, already in the lock through the updater; used by the download in M8b). `status` has an
    Updates line (available / out but no build for this machine yet / the newest / not looked yet / the last look
    failed); login, logout, servers, pair, projects, inbox and plan start with "Update available: x." when a running
    master found one. publish-update.mjs signs and lists master-windows-x86_64 and master-linux-x86_64 builds (cargo
    zigbuild; QANUVI_SKIP_LINUX_MASTER=1 leaves it out; macOS later, on a Mac); not run (publishing waits for the
    owner). Driven: real channel (0.1.25, no master build: "out, but has no build for this machine yet"), a localhost
    test channel with a master build ("0.2.0 is available", the notice before projects list / inbox list), a 404
    channel ("the last look failed (the update channel answered 404)"). Rust 509 (Linux 507), 4 new tests with a
    throwaway test key. Next: M8b (`qanuvi-master update`).
    PICKUP POINT (before, 2026-11-18 11:30): M7b BUILT + DRIVEN LIVE (Windows scratch data
    folder + VPS basevm378 on a throwaway --data folder, removed after; VPS serve runs the M7b server build, M7a one
    kept as .prev): `qanuvi-master inbox list [--all] | show <number or id> | answer <number or id> <choice> [text]`.
    Numbers follow the list's order (open questions first, then newest), each line also shows the id's start; an
    answer by number names the question it answered. Choice by id or label (letter case aside); text only when the
    form has a text box; a secret question (login or password) is refused and sent to the run panel; a note, an
    answered or a closed question says so; a late answer says "Already answered somewhere else.". Answers go with
    by "terminal"; the window's Inbox says "(in a terminal)" (InboxView outcome). A run reads the answer from
    the master's Inbox itself (running or at its next start), so nothing window-only is needed. Code in
    src-tauri/src/master_cli_inbox.rs, 5 tests. Drive seeds questions through `--stdio` inbox_add (scratchpad
    m7b.sh; use printf, Debian sh's echo breaks \n in JSON). Next: M7c plan run <file> + plan list/stop.
    PICKUP POINT (before, 2026-11-18 10:50): M7 AGREED (owner, 2026-11-18): three parts,
    M7a status + projects, M7b inbox (list, show, answer), M7c plan run <file> + plan list/stop; Ctrl+C on a
    followed run only DETACHES the terminal (the run is the master's and goes on; `plan stop` stops it).
    M7a BUILT + DRIVEN LIVE (Windows scratch data folder + VPS basevm378, owner yes): `qanuvi-master status` (not
    started to answer when it is not running) prints version/host, data folder, account (or how to sign in),
    projects, servers + connected, agents and plan runs working, Inbox open + waiting for an answer;
    `qanuvi-master projects list | add <folder> | remove <name, folder or id>` (folder made whole from the
    terminal's own place, "already one of this master's projects", a name two projects share asks for the folder
    or id). Code in src-tauri/src/master_cli_status.rs, 3 tests. VPS runs the M7a server build (Nov 17 one kept as
    .prev). Next: M7b inbox.
    PICKUP POINT (before, 2026-11-18 10:15): M5c PART 2 DONE. w1-w6 FIXED ff2ef72 (owner: fix
    all, in the order proposed) and DRIVEN LIVE on the VPS (release test build), except w3, unit-tested only (no
    failure line to show in the drive). Verify: Rust 487 + 2 ignored, clippy clean, fmt clean, web 496, svelte-check
    0, prettier clean (StartupSettings.svelte formatting from 29c8e38 fixed on the way). Next: keep an eye out for v3;
    then t1, b1/m2a.
    w1-w6 FIXED (2026-11-18):
    - w2: Remotes::answered waits for a link still being made (STILL_STARTING, 35 s) in link's This PC branch and in
      route; "too old" only for a hello without forwards. Test a_link_still_being_made_is_waited_for. Driven: This
      PC's core killed with a VPS file open: intelephense came back by itself (ready, diagnostics).
    - w3: language server error lines kept per machine (#lastErrors), the shown machine's shown; a server ready again
      clears its line. Test in lsp.svelte.test.ts.
    - w4: clientFor waits for a start of the same project and server under way (#opening). Test fails on the old
      code. Driven: the second window started one intelephense.
    - w1: Held.why, set by #lose; the overlay shows it under the title. Test in masters.thispc.test.ts. Driven.
    - w5: "Remove basevm378 from Qanuvi on this computer? It leaves every window. Nothing on the server is changed."
      Driven.
    - w6: a machine window's title follows its shown machine (windows.titleFor, +page effect). Driven: "This PC ·
      Qanuvi" after the fall-back.
    PICKUP POINT (before, 2026-11-18 09:55): M5c PART 2 DRIVEN LIVE (release test build of
    ee63f3b, This PC -> VPS basevm378 over SSH, owner's yes for the VPS; VPS kept its 2026-11-17 server build). All
    six drive checks pass; findings w1-w6 below wait for the owner's word. Next: owner's word on w1-w6, fix, drive w2
    again (kill This PC's core with a VPS file open); keep an eye out for v3; then t1, b1/m2a.
    M5c PART 2 DRIVEN LIVE (2026-11-18):
    - Pass: This PC's master started alone (serve, no window) connected the VPS in under 12 s; the window opened on
      it at once (2/2), with no second SSH link (the local test master's own VPS link is its M3 list, expected).
    - Pass: VPS terminal (bash -l) and intelephense ran under the VPS master's serve, reached through This PC's
      master (the only ssh.exe was under This PC's core); diagnostics showed ("Undefined function").
    - Pass: window closed: the terminal and intelephense ended on the VPS; This PC's master kept its VPS link.
    - Pass: This PC's core killed with the VPS shown: overlay at once, back on in about 3 s (2/2, green).
    - Pass: "Open in a new window" for the VPS joined the held connection (no new link on the VPS).
    - Pass: server removed in the main window: This PC's master let its link go, the second window fell back to
      This PC (1/1), the VPS language servers ended. Server put back afterwards with the same id (test data).
    - Not findings: the first core kill was the 3rd crash in the count (two crash tests left in the test data from
      M5b; the count only resets after CLEAN_RUN), so the watcher stopped; crash file cleared, serve started by hand.
      The nested basevm378 under the local test master is red as "the same computer as basevm378" (n1, by design).
    FINDINGS (await owner):
    - w2 (bug): after This PC's core restarts, the VPS language server fails: "This computer's background part is too
      old to reach its servers. Restart Qanuvi." Cause: Remotes::connect_new puts This PC's new link in the map
      before its hello, with info Null; a call in that gap (the language server restart) goes through Remotes::link,
      which reads forwards != true as "too old". Proposal: link waits for a link still starting (as connect does,
      up to 35 s) and only says "too old" for a hello without forwards; unit test for the gap.
    - w3: that VPS error line stayed in This PC's Problems after switching to This PC (This PC's own servers were
      ready). Proposal: language server status lines belong to their machine and go with a switch.
    - w1: the overlay when This PC's core stops is the generic "connection lost, reconnecting..." (Part E #lose sets
      no error); the reason is only on the switcher row. Proposal: the overlay says it ("Qanuvi's background part on
      this computer stopped; it comes back by itself").
    - w4: opening the VPS in a second window started two intelephense processes on the VPS in the same second; both
      ended with the removal. Not traced yet (one per window, or a doubled start). Proposal: trace, then one per
      machine and project.
    - w5: the Remove server question says "it just leaves this window"; now it leaves This PC's list, so every
      window. Proposal: "It leaves Qanuvi on this computer, in every window. Nothing on the server is changed."
    - w6: the second window's title stayed "basevm378 - Qanuvi" after it fell back to This PC. Proposal: the title
      follows the shown machine.
    PICKUP POINT (before, 2026-11-18 09:40): M5c PART 2 BUILT, NOT DRIVEN LIVE (map below).
    Next: release test build + drive of part 2 (needs a server This PC reaches over SSH or the relay: a "local" test
    master stays the window's own, so it does not take the new way); keep an eye out for v3; then t1, b1/m2a.
    M5c PART 2 BUILT (2026-11-18; verify: Rust 486, clippy clean, server build check clean, web 501, svelte-check 0):
    - This PC's master holds This PC's server connections: it connects the ones marked to connect at start, at its
      start and every minute while away, window or not (master.rs: the app_data early return is gone). Its list keeps
      "local" and "self" entries for the window (servers_save takes them on the app's data) and never tries them.
    - Keys unchanged: the switcher keys of This PC's servers stay their ids, so no one-time move of open state. The
      app's Remotes has a `through` link (THIS_PC): a path whose first machine is not connected in the app goes
      through This PC's master, the whole path passed on with nothing seen yet (that master is this machine; depth
      and the loop guard count as before). link_for makes This PC's link first when needed; coding tools and
      port_forward use link_for.
    - The window (masters.svelte.ts): connect asks This PC's master (callThisPc remote_connect) except for appHeld
      ("local", "self"); disconnect lets This PC's master's connection go; followThisPc follows its remote_watch
      (joins a connection held already, a server ended / back / removed from another window); This PC's own link
      ended: its servers drop with "Qanuvi's background part on this computer stopped, ..." (a shown one held and
      tried again) and the follow is asked for again (up to 6 tries 2 s apart). autoConnect joins the app's and This
      PC's master's open connections.
    - remote.ts linkEnded: a link the app does not hold (one of This PC's servers, or a nested machine) tells its
      terminals it ended (j1 text), as the app's remote-closed does for its own.
    - Found on the way, fixed: (1) two connects to one machine at once (the window's and the master's start-up
      connect) answered the second with an empty hello, which the window would have taken as a failure and dropped
      the master's connection; Remotes::connect now waits for a connect under way (no second connection either).
      (2) A terminal, pipe or port opened through a master (on one of its servers) belonged to that master's lasting
      connection there, so it outlived the window that opened it (also true for nested machines since M3c); the
      passing master now notes it with the asking connection and closes it along the same path when that ends
      (master_servers.rs note_forwarded / close_forwarded).
    Drive next (release test build, scratch project only): a server marked to connect at start is connected by This
    PC's master with no window open (tray: master running, window closed; open the window: it is on at once); a
    terminal and a language server on it through This PC (they run under the server's master, reached through This
    PC's master's connection); close the window: that terminal and language server end on the server; kill This
    PC's core: the server's line says the background part stopped, then it comes back on; a second window joins the
    connection; remove the server in one window: it goes from the other.
    PICKUP POINT (before, 2026-11-18 08:55): M5c PART 1 DRIVEN LIVE (notes below), v1 and
    v2 fixed (aa39649) and DRIVEN LIVE. Next: build M5c part 2 (owner decisions below); keep an eye out for v3; then
    t1, b1/m2a as listed.
    v1 + v2 DRIVEN LIVE (2026-11-18, release test build of aa39649): the window offered Install for the TypeScript
    server ("Downloads typescript-language-server and typescript@5 ...") because the old folder held typescript 7;
    Install put 5.9.3 in, and app.ts showed "Type 'string' is not assignable to type 'number'" (server ready).
    12000 short lines, This PC hidden and shown: the note at the top, then "line 9115" onward.
    v3 (seen once, not reproduced, not fixed): the first start of that test build, with This PC's master stopped
    before it (killed for the build), brought back two terminal tabs titled "Windows PowerShell" that never showed a
    prompt or echoed input, though both shells were running under the new core (spawned in the same second as the
    core started). A new tab worked. A second cold start the same way (window closed, masters killed, start) brought
    both tabs back working. Suspect: the spawn's output stream lost in a race while the window makes This PC's link
    to a master that is just starting (u1 area, desktop.rs connect_link); watch for it at the next cold starts.
    M5c PART 1 DRIVEN LIVE (2026-11-18, release test build of e54df14, scratch project m5c-proj):
    - Intelephense started as a child of This PC's master core (not the app), "ready", flagged an undefined function.
    - Core crashed (crash.js): the open terminal said "Qanuvi's background part on this computer stopped, and this
      shell ended with it."; the watcher started a new core and Intelephense came back under it within about 2 s; a
      second crash, seen in quick screenshots: "Intelephense: restarting", then "starting", with "Checking…" in the
      Problems list instead of "No problems found".
    - Two unsaved files, window closed by its X: This PC's master stayed; on the next start both files came back with
      their text, tab names in italics, and This PC's Inbox had the drafts note.
    - Trimmed note: a terminal with about 400 KB of 200-character lines, This PC hidden by the switcher (local test
      master shown) and shown again: "[Earlier output was trimmed; the most recent part follows.]" at the top.
    - Node debugger on run.js: paused on its breakpoint with variables and call stack; the adapter (dapDebugServer)
      ran under This PC's master core; Stop ended it and the program.
    Found, owner said yes to both proposals, FIXED (2026-11-18):
    - v1: the TypeScript language server never starts: "Could not find a valid TypeScript installation". Its install
      took typescript 7.0.2 (the native one, no tsserver), which typescript-language-server 6.1 cannot use; the same
      when started by hand, so not M5c. Cause: lsp.rs npm lists name "typescript" with no version (typescript and
      svelte servers). Fix: both lists pin typescript@5; a Spec's `needs` names a file an own install must also hold
      (typescript/lib/tsserver.js), so a folder that took typescript 7 counts as not installed and Settings,
      Languages offers Install again, which puts typescript 5 in (unit test v1_...).
    - v2: with short lines the trimmed note is never seen: the master keeps 256 KB (about 5,450 lines of 47
      characters) but the window's terminal holds 5,000 rows, so the note at the start falls out of view. Fix
      (terminal.rs): the kept part is also held to 4,500 rows, a line counted as 1 + its length / 80 (never fewer
      than it takes in a terminal 80 columns wide or wider), dropping whole lines from the front (unit test v2_...).
    - Drive note: terminals end when the window that opened them goes (by design); only the switcher re-attaches.
    Drive tools (scratchpad of session d500cea7): ui.ps1 `text` sends literal text in 8-character pieces and stops
    if focus leaves the test window (plain `keys` sends % as Alt: "% {" opened the window menu and the rest typed
    into another window); restore.ps1 brings back a minimized test window; wheel.ps1 scrolls; burst.ps1 + stack.ps1
    take quick Problems-panel screenshots; pickdir.ps1 fills a folder picker.
    M5c PART 1 BUILT (2026-11-18):
    - Coding tools through the pipes: This PC's language servers and debuggers start on This PC's master through
      its program pipe, as every machine's (desktop.rs tools_host: no `master` = the "this-computer" link, On::this_pc
      so sentences say "this computer"). The app starts no coding-tool program itself any more: Servers::start,
      Adapters::start and their Local variants are gone (lsp.rs/dap.rs keep only the pipe path), as is project_root.
      The inspector and preview stay the window's: This PC's localhost is the window's own, so no tunnel is needed.
    - j1: a terminal whose link ended (This PC's master restarted, or a server's connection went) says so: "Qanuvi's
      background part on this computer stopped, and this shell ended with it." / "The connection to X ended, ...".
      (remote.ts onLinkEnd + linkEndedText; a master ends a connection's terminals and pipes with it.)
    - "Earlier output trimmed": a terminal that dropped its oldest output (over 256 KB kept) says so in grey before
      the kept part when a window attaches again (terminal.rs Outlet.trimmed, TRIMMED_NOTE; plain output, so a
      window of any version shows it).
    - "Checking…": a language server that ended by itself starts again for its open files after 2 s (at most 3 ends
      in 5 minutes, then it stays stopped with why); meanwhile its state is "restarting" and the Problems list says
      "Checking…" instead of "No problems found" (also while a server is starting).
    - i1: a window that goes (closed by hand or by tray Quit, Restart, Restart to update, Reload) first keeps its
      unsaved text as drafts on the shown machine (closing.ts windowGoing, run inside the one close handler; 6 s at
      most) and puts a note in that machine's Inbox ("Unsaved changes to N files kept as drafts on X", files named).
      They come back in their files when the project opens (the B2 path); such a tab's name is in italics with
      "Draft, not saved to disk" until saved, reloaded or undone back to the file.
    Drive next (release test build, scratch project only): a PHP or TS file on This PC gets its language server
    (Settings, Languages shows This PC's servers through the master); a debugger starts; stop This PC's master
    (kill its core: the watcher starts a new one) with a terminal and a file open: the terminal says it ended, the
    Problems list says "Checking…", then the server is back; a terminal with over 256 KB of output, hidden and shown
    again: the trimmed note; two unsaved files, close the window, start again: the Inbox note, both files back with
    their text in italics.
    Left (not built): other windows' unsaved text on Restart (the app ends without their close events); a machine
    held as lost keeps no drafts on close.
    OWNER DECISIONS (2026-11-18, "yes to both"): (1) BUILD M5c part 2: This PC's master holds This PC's server
    connections (connect at start, kept with no window) and the window reaches them through it as nested machines
    of This PC; "local" and "self" stay the window's. (2) The booked restore stays in the app: item done as it is.
    Next: release test build + drive of part 1, then build part 2.
    THE QUESTIONS AS ASKED:
    - This PC's server connections on its master: today the window connects This PC's servers itself (masters
      store), and This PC's master refuses "local"/"self" targets and skips connect_away_servers on the app's data.
      Proposal: This PC's master connects the servers marked to connect at start (so their Inbox and runs are
      followed with no window), and the window reaches them through it as nested machines of This PC, one
      connection each; "local" and "self" stay the window's. This changes the switcher's keys for This PC's
      servers (a one-time move of open state) and is the larger part of M5c.
    - The booked restore: the app applies it at every start before This PC's master starts (also the --background
      start), because the restore includes the window's own WebView storage, which must be put back before a
      window opens. Proposal: keep it in the app (the master never sees a booked restore on the app's data) and
      mark this item done as it is.
    M5b BUILT (2026-11-18; verify: clippy clean, fmt clean, Rust 481, web 481, svelte-check 0). Map:
    - Watcher + core (master_watch.rs): `qanuvi-master serve` is the watcher (watcher.lock, one per data folder; the
      tray icon is its own, so it stays while a crashed core starts again). It starts `qanuvi-master core` (the old
      serve: master.lock, master.json) with QANUVI_WATCHED, in a job object (the core and what it started end with
      the watcher), its stderr kept in master.log (1 MB, then master.log.old; before M5b it went nowhere). Exit 0 =
      clean end (watcher ends, count cleared); 75 = restart asked (Settings, Backups: started again at once, no count);
      anything else = crash: record (exit code, time up, last 40 log lines, its last state note) in
      master-crashes.json, stop note "crash" for its runs, started again after 2 s, then 5 s.
    - Crash limit by count: on the 3rd in a row the watcher checks the known causes (out of memory, database, a
      taken port, crashes within 90 s while the start-up list started) and, with local models on in the brain, asks
      one to read the notes. Safe fix on its own: one start without the start-up list (QANUVI_SKIP_STARTUP), told.
      Anything else: notification + master-crash-report.txt (a click opens it), restarting stops; the tray's "Start
      Qanuvi's background part again" (enabled only then) is the yes; a window's call says so instead of waiting.
      The next core puts the report in the Inbox (problem). Count reset: clean end, a crash after an hour's run, or
      a new watcher (Quit, then open).
    - Stop reasons: the core notes its state every 20 s (master-state.json: at, agents, runs, start-up count); a clean
      end and a restart remove it. A next start with no stop note but a state note: the computer started after it
      ("shutdown": boot time from GetTickCount64, /proc/stat btime, kern.boottime) or not ("ended"). Runs get
      "Stopped when the computer shut down." / "...background part crashed." / "...ended unexpectedly.", each with
      Resume, and stoppedBy in the run.
    - Resume after a restart: plan_run_start keeps its start (no keys) in run-starts/<project>.json. After a
      shutdown, with "Start Qanuvi when Windows starts" on, This PC's master starts the cut-off runs again 30 s after
      start and says so in the Inbox ("Plan run resumed on its own", or "not resumed" with why). With it off, the
      Run panel of a run a shutdown cut off offers the setting (Turn it on / Don't ask again: startOfferOff).
    - The setting is named "Start Qanuvi when Windows starts" ("...when this computer starts" elsewhere).
    - Cleanup (browser.rs): at the master's start, qanuvi-browser-<pid>-* profiles of processes no longer running are
      removed (never a running one's); at its end, also its own; This PC's master removes Qanuvi-*-updater-* folders
      older than 10 minutes.
    - Test build only: hello {"crash": true} aborts the core (scratchpad crash.js), to drive the watcher.
    u1 (found driving M5b, fixed): after the core crashed and was started again, every This PC call of the open
    window said "The connection to the master has ended": the app kept its ended "this-computer" link in its list,
    so link_for never made it again (M5a's "made again after it ended" never ran; no M5a drive restarted the master
    under an open window). Fix (desktop.rs connect_link): when This PC's link ends, the app lets it go (under the
    This PC connect lock), so the next call makes a new one.
    M5b DRIVEN LIVE (2026-11-18, release test build, scratch project m5a-proj with a PLAN.md): app -> watcher (serve)
    -> core for This PC and the local test master; a fake leftover qanuvi-browser-3999999-* profile was removed at
    start; crash 1 (crash.js): record with the log line and state note, the run said "...background part crashed.",
    the core was started again in 2 s and the window went on (after the u1 fix: a new link, no "Sync problem");
    crashes 2 and 3: restarting stopped, notification "Qanuvi's background part crashed 3 times" with the report,
    tray tooltip "...stopped after crashing", "Start Qanuvi's background part again" enabled only then; choosing it
    started a core, cleared the count and put the report in the Inbox (problem); tray Quit ended app, watcher and
    core, wrote "quit", left no state note. Killing the watcher ended its core (job object). Shutdown (watcher
    killed, state note set before boot): the run said "Stopped when the computer shut down." with stoppedBy; the Run
    panel offered the setting (Turn it on wrote the test build's own Run entry); with it on, the next shutdown made
    the master try the kept start 30 s after start and say in the Inbox (and a notification) why it could not go on
    (a start that is not valid was kept on purpose, so no paid run started). Setting turned off again; drive data
    removed; all test processes stopped. Fixed after the drive (unit-tested, not in the driven build): exit codes in
    hex (0xC0000409), and the not-resumed note names the cut-off run's plan with a full stop before "Open the plan".
    Notes: a test build's toasts show as "Windows PowerShell" (no registered app id; as before). The tray's menu is
    not seen by UI Automation, so the drive clicked it by position.
    PICKUP POINT (fresh conversation starts here, 2026-11-18 07:35): M5b BUILT + DRIVEN LIVE (u1 fixed). Next: M5c
    (This PC's coding tools through the pipes, booked restore and server connections on its master, i1 drafts, j1
    ended terminal, "earlier output trimmed", "checking…" for a restarting language server). Later: t1 (Qanuvi-styled
    Quit question), b1/m2a as listed. Not driven: a real plan run resumed after a real Windows restart.
    Left: a real plan run resumed after a real Windows restart is not driven (the resume path is, with a kept start);
    freeing a taken port is not automatic (the program holding it may be the owner's): it goes to the owner.
    PICKUP POINT (fresh conversation starts here, 2026-11-18 06:30): M5a DRIVEN LIVE, s1-s5 found and fixed (notes
    below; each fix driven on a release test build; verify: clippy clean, Rust 470 + quoting test, web 479,
    svelte-check 0). Next: M5b (watcher + core split, crash limit, runs resume after a Windows restart, self-cleanup).
    Later: t1 (Qanuvi-styled Quit question). Test build state: Notifications back to "problems", fake work cleared,
    all test processes stopped.
    PICKUP POINT (2026-11-18 06:00): M5a BUILT (b633c95 cut-over, 4f9f15f notices
    + r1 fix), NOT YET DRIVEN LIVE with the r1 fix. Next: drive M5a live on a scratch project (every page, terminal,
    Inbox, Settings; last window closed with work running says what keeps going; with no window, the master's own
    notification; tray Quit asks first; "Stopped when Qanuvi was quit" on the run after relaunch), then M5b.
    State: a release test build of 4f9f15f was building into target/release (rebuild if it is not there); all test
    build processes were stopped first. Scratch project for the drive: m5a-proj (git, docs/note1..40.md) in the
    scratchpad of session dac38bdc, already in This PC's test project list (d05f80a7). Drive helpers there: ui.ps1,
    procs.ps1, mcall.js, hop.js, tdb.js (fake-work <projectId> adds a waiting session so the master reports work;
    fake-run adds a running run record; clear-work removes both).
    M5a BUILT (2026-11-18; verify: clippy clean, Rust 468, web 479, svelte-check 0): the window is a viewer.
    remote.ts sends every This PC call to its master over the "this-computer" link (callThisPc; desktop.rs makes it
    on the first call under a lock, again after it ended); WINDOW_ONLY = master.rs INTERFACE (a test checks both
    ways). desktop.rs lost the second core (167 commands, desktop_runs.rs; 43 + notify_notice left), opens no
    database; lsp/dap find a This PC project's folder through the master. The master answers core_decide,
    knowledge_fetch, attachment_page, attachment_image, inbox_import, plan_data_import, and plan_data_watch (every
    window hears plan data changes, a run's too). Runner follows This PC runs on plan_run_watch; the app still
    follows This PC's Inbox and raises its notifications. Parity test: GAPS gone, the app answers only INTERFACE +
    links. Part 2: This PC's master marks its own leftover sessions/runs (the M4a "leave the app's runs" rule ended);
    update/Quit/restart write why (master-stopped-why), the next start puts it in the run's message; Restart,
    Restart to update and tray Quit (MessageBox) ask with "N agent sessions and N plan runs"; no quiet install on
    close while work runs; last window closing with work: notice "... keep going on this computer. Quit Qanuvi from
    the tray icon to stop them."; no window following: master_notify.rs shows the item itself (click opens Qanuvi
    at it). Hop measured (direct TCP vs the stdio bridge, 20 calls each): +0.1 to +2.5 ms per call (hello 0.76 vs
    0.97, files_search 24.6 vs 26.1, git_status 123.6 both), so no direct link is needed.
    r1 (found driving the first build, fixed in 4f9f15f): the M3c loop guard refused the "this-computer" link (same
    machine id as the app): "That path comes back to a machine it has already passed". Only that link now starts
    the path at This PC; any other link back to this machine is still a loop.
    Left for later: This PC's server connections at start (master skips them on app data, M5c); a crash or power
    cut leaves no stop reason yet (M5b); tray Quit confirm is Windows-only (others with M5b's watcher).
    M5a DRIVE (2026-11-18, build of 4f9f15f): pages, terminal (typed, output back), Explorer, Inbox (shared, both
    machines), Settings Servers all through the master; last window closed with work: the notice came; no window:
    the master's own notification came (driven with omniroute_look; inbox_add from a connection is the window's own
    note and is told by the window, by design) and its click opened the test build at the item; tray Quit asked
    (MessageBox of the master) and Yes ended master and window and wrote "quit". Found and fixed:
    s1: two This PC masters on one folder: the app's launch (M4a start_this_pc) and the first This PC call (stdio
    bridge ensure_service) each started one; both kept running the background jobs on one database. Fix: serve takes
    an OS lock on master.lock (File::try_lock) and a second one ends at once; both start paths skip the spawn while
    the lock is held (and no longer delete a starting master's master.json); backups and data moves skip the lock.
    s2: "1 agent session ... to stop them" / "They can be resumed": it/they by count (notice, tray Quit, Restart).
    s3: an unread OmniRoute sign-in note was told again at every look (30 minutes, and each start through the
    window's look): add_new now says not new for the same note again while it is open (refreshed, not told).
    s4: tray Quit closes the windows as by hand, so the last one said "... keeps going on this computer" while Quit
    stopped it. Fix: the app marks quitting (app_quitting, an INTERFACE command; reset after 30 s, as a cancelled
    close keeps everything running) and the window says nothing then.
    s5: tray Quit, Yes: the window closed but This PC's master stayed (no stop reason): two connections of the gone
    app still counted as a window following. Their handles had passed app -> local-master bridge -> that second
    master's long-lived service (Command starts children with every inheritable handle); stopping it closed them.
    Fix: on Windows the detached service starts through CreateProcessW with no inherited handles (spawn_alone, own
    argument quoting with a test); windows-sys gains Win32_Security.
    Driven again on the fixed build: one master on the folder (master.lock), the fake run said "Stopped when Qanuvi
    was quit. Resume goes on from here." after relaunch, no sign-in note told again at start.
    Later (owner, 2026-11-18) t1: the tray Quit question is a Windows MessageBox (the master has no window); ask
    through an open window's own Qanuvi dialog, and with no window a small Qanuvi-styled one.
    PICKUP POINT (fresh conversation starts here, 2026-11-18 05:30): M5 AGREED (docs/MIGRATION-VIEWER.md M5:
    M5a cut-over with the second core deleted in the same pass, no old-core adapting; M5b watcher + crash limit of 3
    then the brain investigates, runs resume after a Windows restart when "Start Qanuvi when Windows starts" is on;
    M5c coding tools, i1, j1, notices). New M10: account gate + same-account device discovery with one-click pairing.
    Added to M5b (owner, 2026-11-18): Qanuvi cleans up after itself: qanuvi-browser-* profiles removed when the
    browser/master ends and stale ones (dead pid) at start; old Qanuvi-*-updater-* folders removed after an update.
    (The qc_src_* and qctest_* temp folders are the qCommerce test suite's, not Qanuvi's.)
    Next: build M5a. Starting map (measured 2026-11-18): the window already reaches This PC's running master through
    remote_connect target "self" (qanuvi-master --stdio = master::stdio_bridge into the service), so M5a = keep a
    "this-pc" link up from launch and make routed()/callOn() in src/lib/remote.ts send null (This PC) to it instead
    of the app's invoke; the ~44 direct invoke() calls in stores/components are mostly WINDOW_ONLY already (check
    each); runner.svelte.ts + masters.svelte.ts #listen and desktop_runs.rs app.emit go over to the master streams;
    then remove the core from desktop.rs (224 fns, handler list at generate_handler!) down to master.rs INTERFACE,
    and GAPS (core_decide, knowledge_fetch, attachment_page, attachment_image) move to the master. The hop through
    the stdio bridge is two hops (window -> bridge process -> TCP); measure it, and a direct TCP link is the fix if
    it is slow. Free RAM was 4.7 of 15.3 GB: run the Rust builds alone.
    PICKUP POINT (before, 2026-11-18 04:30): M4c BUILT + DRIVEN LIVE, so M4 is DONE. Next: M5
    (the window becomes a viewer: This PC's calls go through its master; measure the hop). State: test build of M4c
    in target/release; test window, This PC's test master and local test master all stopped; no sign-in entry.
    M4c BUILT + DRIVEN LIVE (2026-11-18; verify: clippy clean desktop + server build, Rust 463, web 476): This PC's
    master shows a tray icon while it runs (master_tray.rs, part of the qanuvi-master program only: tao + tray-icon
    on its main thread, connections on another; the library and its tests never link the tray, whose comctl32 v6
    calls need the app manifest). Menu "Open Qanuvi" (also a left click): a running app shows its window through its
    link (app_follow, follow_this_pc_app), else the app beside the master starts; "Quit Qanuvi": the app's windows
    close as by hand, then the master ends (end(); a window still open after 20 s keeps everything). The last
    window closing no longer stops the master (remote access or not). Settings, Start-up (This PC only): "Start
    Qanuvi in the background when I sign in", off by default (sign_in_start.rs: the user's Run key on Windows,
    ~/.config/autostart on Linux, a LaunchAgent on macOS; the entry is the setting; a test build has its own entry;
    written again at start when the app moved) runs `qanuvi --background`: the app starts This PC's master and
    ends with no window. The window stops the master before an update installs (this_pc_master_stop, both install
    paths). Also: a connection that ended gets a refusing writer, so its followers count as gone at once (a closed
    socket can take a first write on Windows).
    Driven: launch -> master + tray icon "Qanuvi (test build)" (Windows 11 puts it in the hidden-icons overflow);
    window minimized + icon clicked -> window back in front, no second app; menu Open with the app running -> same;
    last window closed -> app gone, master + icon stay and answer; Open with no app -> the app starts on the same
    master; Quit -> window, master and icon gone (no ghost icon); setting on -> Run entry "Qanuvi (test build)" =
    "...\target\release\qanuvi.exe" --background (installed entry untouched); that command run after Quit ->
    launcher ends at once, master + icon up, no window; setting shown On again, turned off -> entry removed. Not
    driven: an update installing (needs a published one), Linux/macOS sign-in entries, a cancelled close during Quit.
    Drive tricks: the tray menu is owner-drawn (no UIA items): click rows by position in the #32768 window of the
    master's pid; match the icon as a Button (its tooltip has the same name).
    PICKUP POINT (before, 2026-11-18 03:00): M4b BUILT + DRIVEN LIVE. Next: M4c (tray icon
    whenever the master runs with "Open Qanuvi" and "Quit Qanuvi", the master staying after the last window closes,
    "Start Qanuvi in the background when I sign in" off by default, the app stopping the master before an update
    installs). State: test build of M4b in target/release; test window, This PC's test master and local test master
    all stopped; test start-up list empty again.
    M4b BUILT + DRIVEN LIVE (2026-11-18; verify: clippy clean, Rust 462): This PC's master runs the background jobs
    on the app's data (start_own no longer skips them: start-up list, Claude Code update, OmniRoute watch, memory
    disk sync; serve's daily backup into the app's own backups folder with its WebView storage, backup_place). The
    app no longer runs any of them (its Startup is gone from AppState). Still the app's until M5: running sessions and
    runs (mark_interrupted), a booked restore, This PC's server connections (the master skips connect_away_servers
    on app data), the idle agent closing. The window's startup_* commands go to the master (master::this_pc_call,
    waits up to 10 s, never starts a second one); the app follows the master's inbox_watch
    (follow_this_pc_inbox) and hands each change to its own "qanuvi-inbox" event and notifications.
    Driven: start-up item started under This PC's master (not the app); Settings, Start-up showed it running,
    Restart/Off/On went through the master; a note added on the master showed in the Inbox at once (badge 1, under
    This PC); the master's OmniRoute watch put a "sign in again" notice in the Inbox 60 s after start; memory disk
    sync ran at 90 s; with the backup made due, the master made it at 2 min in Documents/"Qanuvi backups (test
    build)" with its window folder; last window closed -> master and start-up item gone. Remote access is the
    master's relay already (since M4a). Not driven: remote access on with the window closed (jobs go on).
    PICKUP POINT (before, 2026-11-18 02:40): M4 agreed (docs/MIGRATION-VIEWER.md M4: M4a-M4c,
    master keeps running with a tray icon after the last window closes, "Start Qanuvi in the background when I sign
    in" off by default, the hop measured in M5). M3c depth 3 + loop guard is later step M3d. Next: M4b (background
    jobs move to the master; reduced mark_app_data mode ends), then M4c (tray icon, Quit, start at sign-in, stop
    before update). State: test build of M4a in target/release; test window, This PC's test master and local test
    master all stopped.
    M4a BUILT + DRIVEN LIVE (2026-11-18; verify: clippy clean, Rust 461): the app starts `qanuvi-master serve --data
    <app data>` at launch (master.rs start_this_pc, the exe beside the app) or reuses a running one. A master on
    the app's data skips mark_interrupted and the booked restore (they were cutting the app's live runs off when
    remote access started a master; test m4a_this_pcs_master_starting_leaves_the_apps_running_runs_alone). Until
    M4c: the last window closing stops it unless remote access is on; app_restart stops it first (move/restore).
    Driven: no master before -> one on the app data after launch (window normal on the same DB); a running run
    survived a master restart; last window closed -> master gone (remote access off); remote access on -> it stays;
    a second launch reused the running master (one serve). Not driven: app_restart after a data move or restore.
    PICKUP POINT (before, 2026-11-17 20:15, after d57f56b): open items in order:
    (1) M3c depth 3 + loop guard moved to later step M3d (owner, 2026-11-17: when they arrange another machine); the
    migration goes on with M4. (2) m2a seen live on a master's Account page (fix in, only checked by svelte-check).
    State: test build of d57f56b in target/release; test window and local test master stopped. VPS basevm378 runs
    the server build of d57f56b (cross-built 19:50, 9.9 MB, zigbuild -j 2), swapped into ~/.local/bin, the Nov 16
    build kept as qanuvi-master.prev; serve started again with setsid (no service manager there). Drive scripts
    ui.ps1/go.ps1/room.ps1/inwin.ps1 now also in the scratchpad of session 4ca626fb. Test project b1proj (that
    scratchpad) has a seeded stopped-by-budget run in the test app's database.
    b1 BUILT + DRIVEN LIVE (d57f56b, 2026-11-17; verify green 476 + 460): the run record keeps budgetReached when the
    run stops on its own budget; the status line says "Stopped: budget $1.00 reached" (the message is not repeated);
    a Raise budget button next to Resume opens the run's settings with the budget box focused and selected; the start
    panel warns when the budget is below what a step usually costs on the first step's model (costs.ts usualStepCost:
    same kind of job first, else every kind on that model; failed tries count in), "the run will likely stop after
    step N". Driven with a seeded record ($1.16 Opus step, $1.00 budget): line, button, focus, warning shown with
    Opus for every step, gone at $2. Rust scenario b1_a_run_stopped_by_its_budget_says_so_on_the_record.
    m2a FIXED (d57f56b): explicit space after "password." before the master-only sentence on the Account page.
    PICKUP POINT (before, 2026-11-17 19:45, after 3fb5878): open items in order:
    (1) M3c depth 3 + loop guard driven live when the owner provides another machine (window -> local test master ->
    VPS -> new machine, and a path back to a machine already passed). Not driven yet either: a master disconnected on
    purpose while one under it is shown, unpair (removed) live, a language server on a nested machine (unit-tested;
    same Routed link as preview, which was driven). (2) VPS server build swap before the VPS is a middle hop (owner
    yes in the plan; the VPS was only the end of the path so far, binary 0.1.23 unchanged). (3) m2a waits for the
    owner's yes. (4) b1 after M3. State: test build of 3fb5878 in target/release; test window and local test master
    stopped (start with go.ps1; the master exe is locked while its service runs: `qanuvi-master stop --data <folder>`).
    Drive scripts ui.ps1/go.ps1/room.ps1 now in the scratchpad of session 81c1f6f0. Test data: This PC's basevm378
    has "Connect when Qanuvi starts" on, so at start the local test master's basevm378 shows as "the same computer as
    basevm378"; disconnect the direct one on This PC's Servers screen to use the path.
    M3c DRIVEN LIVE at depth 2 (2026-11-17, test build of 3fb5878; window -> local test master -> VPS basevm378 with its
    binary unchanged): the VPS shows under the test master on a guide line; with the direct connection off the VPS has
    exactly one `qanuvi-master --stdio` (the test master's) and a terminal there answered hostname through it; preview
    of the VPS's localhost:8765 loaded through two hops (test web server removed after); button "DESKTOP-R… ›
    basevm378"; the window's link to the test master ended by itself -> the shown VPS held "connection lost,
    reconnecting" and came back through it by itself. Findings, all FIXED and driven: n1 the same computer direct and
    nested showed twice (now "the same computer as basevm378", picking it shows the direct one; race-proof); n2 the
    button cut the shown machine's name; n3 a master said "on" with no hello while a connection opened and the window
    took it as on without reading it (no twin check, projects, Inbox); n4 a master that went away wiped its list, so a
    shown one under it could not be retried and This PC was shown (list kept, its servers show off under it); n5 a
    machine no longer known was named by its path of ids ("that machine" now). Disconnected on purpose: the branch is
    off and nothing is tried again. Verify green 474 + 459.
    M3c BUILT (2026-11-17, 266d2a5; verify green 469 + 459): owner answer to the plan questions: a nested
    machine behaves exactly as This PC's servers do (off/error rows too, connect on click, always shown, preview and
    language servers too); depth 3 + loop guard driven when the owner provides another machine. Wire: remote_call
    takes via + seen; a master refuses a loop (its id in seen) or a path deeper than 3 below the window, then passes
    the call on (master_servers.rs route_for); hello says forwards: true and a path never goes through a master
    without it (an older one would run the call on the wrong machine). remote.rs Routed is a pipes::Link, so calls,
    port forwards (preview) and language servers go along "vps1/vps2". Window: masters.svelte.ts follows each
    forwarding master's servers_list + remote_watch (nested, find, tree, route, reach); nested machines connect
    through their master, a lost shown one is held and retried, one removed says why, a branch goes with its master,
    the same computer twice shows once ("the same computer as ..."); switcher rows indented on a guide line, button
    shows "VPS › build-box". Test masters.nested.test.ts (7). Design in docs/MIGRATION-VIEWER.md M3.
    PICKUP POINT (before, 2026-11-17 17:30, after 540e646): open items in order:
    (1) M3c: a call to a nested machine carries a path (["vps1", "vps2"]), each master forwards it with its streams
    (master_servers.rs remote_call is the one-hop start), loop guard by lasting id, depth at most 3, unpair drops the
    whole branch at once, nested dropdown in the switcher (docs/MIGRATION-VIEWER.md "M3"); plan the path format and
    the dropdown with the owner before code. (2) Before M3c is driven on the VPS: swap in a new server build there
    (owner yes is in the plan; cargo zigbuild for Linux, `pkill -x qanuvi-master`, keep .prev). (3) m2a, one-space fix
    in src/lib/components/AccountView.svelte, waits for the owner's yes. (4) b1 (budget stop looks blocked) after M3.
    State now: local test master (dev.qanuvi.desktop.source/local-master) runs the 540e646 master build and keeps
    basevm378 as its own auto-connect server (SSH, ~/.ssh/vps_key). Test build: owner rule, no asking for build,
    test master stop/restart/swap, launch or drive (clear stray old-session processes and check RAM first); the master
    exe is locked while its service runs (`qanuvi-master stop --data <folder>` first; closing the test window with
    taskkill /T ends the service too). Drive scripts ui.ps1 + go.ps1 (setval, invoke, clickxy, shot; an "invoke" by
    name can hit a tooltip, click buttons by position) in the scratchpad of session 42bf40bb.
    M3b DRIVEN LIVE (2026-11-17, test build of 024d1da; window -> local test master -> VPS basevm378 over SSH with
    ~/.ssh/vps_key, the VPS binary unchanged): terminal `qanuvi-master servers add root@165.99.219.195 --name VPS
    --program ~/.local/bin/qanuvi-master --key ~/.ssh/vps_key` connected (basevm378, master 0.1.23), duplicate name
    refused, list shows connected; the local test master shown in the window lists its OWN servers (This PC's list
    stays on This PC's screen); Disconnect/Connect from the window seen by the terminal; rename to empty = its
    computer name; colour + lasting id kept on the master; service stopped -> new service connected the VPS by itself
    and the window followed the new connection; Remove ended the master's own ssh link at once; Add form connected
    and named it basevm378. Finding s1 (a "local" server was kept and retried every minute) FIXED: servers_save on a
    master refuses "local"/"self", driven live. Next: M3c (forwarding along the path, loop and depth guard, nested
    dropdown); the VPS server build swap (owner yes, plan) comes before M3c is driven there.
    M3b BUILT (2026-11-17, 024d1da + s1 fix): remote.rs shared by app and master (end callback instead of window
    events; a deliberate close no longer reports itself ended); pairing_keys.rs (credential store on desktop builds,
    0600 pairing-keys.json on the server build); master_servers.rs answers servers_* and remote_* plus remote_states
    and remote_watch, removing a server ends its connection at once, refuses this machine itself, connects its
    start-up servers at start and every minute; master_cli.rs: `qanuvi-master servers list|add|remove` and
    `qanuvi-master pair` (thin clients of the service, window's name rules); Servers screen of a shown master =
    MachineServersView (machineServers store). Parity test was red since M3a (servers_* missing on the master), green
    again; remote_* gaps closed.
    M3a DRIVEN LIVE (2026-11-17, test build of f2e193a on the source data folder): store v12 -> v13; the window's
    2 servers (local test master, basevm378) moved in once, same order and fields, flag set; both connected (2/2),
    switcher shows them; a colour change on the Servers screen reached the servers table; after a restart the list
    came from the database, the flag kept its first time (no second move); no error line on the Servers screen.
    Next: M3b (remote_* on the master + pairing from a master; remote.rs split from the window's events, a writable
    0600 key file for the server build's pairing keys).
    M3a BUILT (2026-11-17, f2e193a): This PC keeps its server list in its database (store v13:
    servers + flags tables, servers.rs; servers_list/import/save/remove), the window's old list moved in once,
    window copy kept current (going back to 0.1.25 still finds the servers). M3 decisions in docs/MIGRATION-VIEWER.md
    (e7a5a59): depth 3, unpair at once, pairing from window and master terminal, VPS server build first (owner yes).
    PICKUP POINT (before, 2026-11-17 13:20): open items in order: (1) m2a, one-space fix in
    src/lib/components/AccountView.svelte, waits for the owner's yes; (2) plan M3 (docs/MIGRATION-VIEWER.md "M3. Each
    machine owns its server list") with the owner before any code; (3) the VPS still runs the 2026-11-16 server build
    (no M2): swap in a new server build before M3 is driven there. Test build: npx tauri build --no-bundle with
    CARGO_BUILD_JOBS=2 (about 6 min; check free RAM first), start it with QANUVI_ACCOUNT_SITE and QANUVI_ACCOUNT_CLIENT
    set (values in the entry below) so the window and the local test master use the local Qlopy site; drive with ui.ps1 (UIA,
    screenshots), the owner clicks Allow in the browser. Scripts (ui.ps1, go.ps1, room.ps1) in the scratchpad of
    session 21bcfc6b. No release is waiting.
    RESUME HERE (newest, 2026-11-17 13:15): M2 DONE. Window parts DRIVEN LIVE in a test build of main (QANUVI_ACCOUNT_SITE
    https://aloelo.dev/qanuvi, QANUVI_ACCOUNT_CLIENT qpc_5caa74da17fe15e9 set on the window; the local test master was
    started by the window with the new binary and took the same settings). PASS: This PC Account page signed out ->
    Sign in -> browser opened by account_wait -> owner Allow -> admin shown; local test master shows its own signed-out
    page while This PC is signed in (title "Qanuvi account: DESKTOP-R8G3PAK (test)", kept-on-that-machine note,
    qanuvi-master login note) -> Sign in -> browser -> Allow -> signed in there; Sign out on the master leaves This PC
    signed in; Cancel during a sign-in closes the box with no error. VPS not driven (still the 2026-11-16 server build,
    no M2). Finding m2a (awaits owner): on a master's Account page two sentences run together ("password.The browser
    opens..."). Note: the page copy says qanuvi.com also when a test site is set (only seen with a test site). Next: m2a
    (owner), then plan M3 with the owner before code.
    RESUME HERE (before, 2026-11-17 12:45): runner fix d2799fe (handed-over steps not redone; also 0173046 on
    old-infrastructure) PUBLISHED as 0.1.25 (7d39517, tag v0.1.25, owner "publish anyway"). M2 DRIVEN LIVE on the
    master side against the local Qlopy site https://aloelo.dev/qanuvi/ (disk E:/Wnmp/nginx-bins/default/www/qanuvi,
    mkcert certificate; JWT, OAuth, application passwords on; app "Qanuvi" client id qpc_5caa74da17fe15e9 with redirect
    http://127.0.0.1:47213/callback; the site makes its own client ids, so qanuvi.com's real id goes into CLIENT_ID when
    the app is registered there). Fix found: account calls now trust the system's certificate store (ureq
    platform-verifier), the built-in list refused the mkcert site. PASS: qanuvi-master login (wrong password refused,
    right one signed in via password), master account_me reads it, account_sign_out, account_begin -> browser Allow ->
    code on 127.0.0.1:47213 -> account_finish (wrong state refused, finish once only, via browser), account_me,
    qanuvi-master logout revokes. Site side: default theme /account/machine-passwords (inc/machine-passwords.php,
    template-machine-passwords.php; one-use form token, so a reload makes no second password), driven with
    Playwright. NOT yet driven: the window's own parts (Account page per shown machine, account_wait opening the real
    browser) in a test build with QANUVI_ACCOUNT_SITE + QANUVI_ACCOUNT_CLIENT; local test master was stopped for the
    release build (owner yes), restart it for that. Then M3.
    RESUME HERE (before, 2026-11-17 07:00): r1 DONE (edb21c1, owner yes): a failed reload of a machine's debugger
    statuses keeps the old list and says "Could not check <machine> again: <error>" with Check again (Settings >
    Languages). M2 CORE BUILT (76a137e, owner agreed the plan as proposed; NOT driven live): account.rs in both builds;
    each machine keeps its own sign-in (This PC: credential store `account:<site>` as before, an old bare refresh
    token still reads; a desktop-build master: `master-account:<machine id>:<site>`; a server build:
    `account-sign-in.json` in its data folder, 0600, in backups' SKIP). Browser sign-in split: `account_begin` +
    `account_finish` on the shown machine (verifier stays there), `account_wait` + `account_cancel` in the window
    (INTERFACE; browser + 127.0.0.1:47213 here). `qanuvi-master login [--site] [--password-stdin]` (application
    password as HTTP Basic, checked at users/me first, no echo) and `logout`. Account page + status bar show the
    shown machine's account (`via` browser/password). settings.account.signedIn is no longer read (left in the
    settings shape). Tests: 9 account (file store, stand-in site: begin/finish, wrong state, finish once, refresh,
    revoke, app password, 0600), parity test with account_* off GAPS, window account.svelte.test.ts; 453 TS tests.
    NEXT: drive M2 live against a local Qlopy site (owner builds it: REST + JWT + OAuth on, app `qanuvi-desktop`
    with redirect http://127.0.0.1:47213/callback, application passwords on, a test user with one): This PC
    browser sign-in, the local test master browser sign-in, `qanuvi-master login` on the local test master; test
    builds get QANUVI_ACCOUNT_SITE (window and master). Then M3.
    RESUME HERE (before, CHECKPOINT 2026-11-17 06:45, fresh conversation): M1 IS DONE (M1a pipes, M1b language
    servers, M1c preview, M1d debuggers, all driven live on the local test master and the VPS). Open, in order:
    (1) r1, owner to decide (proposal in the entry below: a failed debugger-status reload says so with Check again,
    instead of silently keeping the old list); (2) M2, the account on the master (docs/MIGRATION-VIEWER.md "M2"):
    account_* to the master with the refresh token and application password in that machine's secret store,
    `qanuvi-master login`, browser sign-in handed to the shown machine, Account page per shown machine; the site side
    (qanuvi.com "Machine passwords" page) is the owner's, separate from this repo. Plan M2 with the owner before code.
    Release grouping: M1 alone, so a release of M1 can come before M2 (owner's yes to publish).
    State: test window = release test build 06:15 (src-tauri/target/release/qanuvi.exe); local test master still on
    its 05:17 build (`qanuvi-master serve --data %APPDATA%\dev.qanuvi.desktop.source\local-master`, running; its exe
    was in use, so the last two builds did not replace it). VPS basevm378 (165.99.219.195, ssh -i ~/.ssh/vps_key
    root@): server build from c57122a (.prev kept), Node 26.13.1 in /usr/local/bin (Debian 18 in /usr/bin), PHP CLI
    8.2 and 8.5 with Xdebug (fpm/cgi without it: it is a hosting server, never let an apt install enable PHP
    extensions for fpm), debug adapters in /root/.local/share/qanuvi/debug-adapters, test project
    ~/qanuvi-test/m1b-vps (index.php, app.js). Before any build: check free RAM (about 4 GB free today), cargo -j 1
    or 2, timeouts on tests.
    RESUME HERE (before, 2026-11-17 06:40): M1d DRIVEN LIVE ON THE VPS, ALL PASS. Owner put Node v26.13.1 (nodejs.org
    LTS, SHA256 OK) in /opt with links in /usr/local/bin (Debian's 18 stays in /usr/bin). basevm378: js-debug run by
    the VPS's `qanuvi-master serve` with /usr/local/bin/node (port 0, through the tunnel); paused at app.js:3 (a 2,
    b 3, call stack), Continue -> "total is 5", "Debugging ended.", nothing left on the VPS.
    Finding r1 (owner to decide): the already open window kept showing js-debug's "needs Node.js 20 ... 18.20.4"
    after Node 26 was in place; a fresh window showed it right. dap.loadStatuses falls back to the cached list when
    the statuses call fails, silently, so the call most likely failed once (cause not seen). Proposal: on a failed
    reload keep the list but say "Could not check <machine> again: <error>" with a Check again button.
    NEXT: owner decides r1, then M2 (account on the master).
    RESUME HERE (before, 2026-11-17 06:20): q1 + q2 FIXED (owner: "yes to both") and DRIVEN LIVE (release test
    build 06:15; the local test master still runs its 05:17 build, its exe was in use; nothing master-side changed).
    q1: each debugger spec has `min_node` (js-debug 20, PHP Debug 0); dap.rs `usable_node` asks the machine's
    `node --version` through `host.run` and refuses with "This debugger needs Node.js 20 or newer; that machine has
    18.20.4. Install a newer Node.js from nodejs.org on that machine first." in the status (Languages shows it also
    for an installed debugger), Install, and both starts. A master's adapter that ends before naming its port now
    says "<label> ended before it was ready. It said: <its error line>" (`adapter_said`). Driven: basevm378 Languages
    shows the line under js-debug v1.140.1; F5 on app.js shows it and starts nothing on the VPS; This PC (Node 24)
    unchanged. q2: `xdebugExample` (dap/protocol.ts): .so example when the shown machine's PHP path is a Linux/macOS
    one; driven (VPS ".so", This PC keeps its .dll). Found while driving, fixed: a start error stayed in the Run and
    debug sidebar after switching to another machine; dap store keeps its machine and `stopMachine` clears it
    (driven: gone on This PC; the Debug console keeps its history lines, as before). Checks: clippy clean, 442 Rust
    lib tests, 449 vitest (new dap.svelte.test.ts), svelte-check 0, prettier.
    Node 20+ on the VPS: NOT DONE. The install (nodejs.org v26.13.1 LTS, SHA256 checked, /opt + links in
    /usr/local/bin ahead of Debian's 18) was blocked by the session's permission check; it changes `node` for the
    whole hosting server, so it waits for the owner (run it themselves, or allow it). Then js-debug on the VPS
    (breakpoint app.js:3), then M2.
    RESUME HERE (before, 2026-11-17 06:00): M1d DRIVEN ON THE VPS (owner's yes for the server build and for PHP with
    Xdebug). Server build cross-built from c57122a (zigbuild -j 1, 9.6 MB, 5m 51s), swapped into ~/.local/bin on
    basevm378 (old one kept as qanuvi-master.prev, old serve ended with pkill -x, window reconnected by itself).
    VPS note: basevm378 is a hosting server (nginx, php-fpm 5.6 to 8.5 all running). `apt install php-xdebug` went
    to PHP 8.5 and switched it on for 8.5 fpm and cgi too; undone at once (phpdismod -s fpm/cgi, fpm reloaded; it
    had Xdebug about a minute, no nginx site points at 8.5). The CLI `php` is 8.2, so php8.2-xdebug was installed
    for the CLI only (fpm and cgi marked disabled_by_admin first, so the 8.2 sites never loaded it). Now: Xdebug
    3.5.3 in the 8.2 and 8.5 CLIs only.
    PASS: basevm378 shown -> Settings, Languages lists the VPS's own statuses (both debuggers "Not installed";
    "PHP 8.2.35 NTS x64 · Xdebug 3.5.3 (loaded by php.ini)", /usr/bin/php). Install put js-debug v1.140.1 and PHP
    Debug v1.40.2 into /root/.local/share/qanuvi/debug-adapters (host_fetch; versions through host_read). PHP Debug
    in project ~/qanuvi-test/m1b-vps (app.js added): phpDebug.js run by the VPS's `qanuvi-master serve`, php with
    xdebug.client_host=127.0.0.1 on the VPS; paused at index.php:3 ($name "42", call stack greet, {main}).
    Switching to This PC ended the session and both VPS processes.
    FAIL (findings, owner to decide): q1: js-debug on the VPS stops at once with "JavaScript debugger (js-debug)
    did not say which port it listens on." Cause: js-debug 1.140 needs Node.js 20+ (RegExp `v` flag); the VPS has
    Debian's Node 18.20.4 (apt offers nothing newer). The adapter's own error (SyntaxError ... flags 'v') is lost.
    Proposal: dap.rs keeps the adapter's last stderr lines and shows them when it ends before naming its port; the
    js-debug status and install check the machine's Node version first ("needs Node.js 20 or newer; basevm378 has
    18.20.4"). To pass js-debug on the VPS: Node 20+ there (nodejs.org build or NodeSource repo; owner's yes).
    q2 (small): the Xdebug file box's example is a Windows .dll path even when the shown machine is Linux; show a
    .so example there. NEXT: owner decides q1/q2 (and Node 20 on the VPS), then M2 (account on the master).
    RESUME HERE (before, CHECKPOINT 2026-11-17 05:35, fresh conversation): M1d DRIVEN LIVE on the local test
    master, ALL PASS (owner's yes to stop the old test master and to download the adapters). The test master was
    stopped, both binaries rebuilt (release, from 24a470e), and the master started again on the new build
    (`qanuvi-master serve --data %APPDATA%\dev.qanuvi.desktop.source\local-master`; still running). Settings,
    Languages with the master shown: both debuggers "Not installed" there (This PC has them); Install downloaded
    js-debug v1.140.1 and PHP Debug v1.40.2 into local-master\debug-adapters (host_fetch); versions shown through
    host_read. PHP check ran on the master; its own Xdebug file (E:/DevTools/xdebug) set there -> Xdebug 3.5.3.
    Scratch project E:\qc-scratch\m1b-drive (app.js added): js-debug started by qanuvi-master with port 0,
    reached through the tunnel; paused at app.js:2 (a 2, b 3, call stack), continue -> "total is 5", debugging
    ended, adapter gone. PHP: PHP Debug started by qanuvi-master, php with the master's Xdebug file, paused at
    index.php:3 ($name "42", Xdebug's call back on the master's localhost); switching to This PC ended the session
    and its processes. NEXT (fresh conversation): M1d on the VPS (Node there; PHP and Xdebug on the VPS need the
    owner's yes), then M2 (account on the master).
    RESUME HERE (before, 2026-11-17 05:45): M1d BUILT, NOT YET DRIVEN LIVE (details in docs/MIGRATION-VIEWER.md,
    debugger bullet). Checks green: clippy clean, 440 Rust lib tests, 446 front-end tests, svelte-check 0 errors,
    prettier. Driving it live needs the local test master to run the new master build (host_fetch, host_read) and
    the adapters on that master (download from GitHub, or a copy of This PC's test-data adapters). NEXT: drive M1d
    on the local test master (js-debug on a Node file, PHP Debug), then the VPS.
    UPDATE 2026-11-17 05:25 (owner: "fix and go on"): p1 + p2 FIXED and DRIVEN LIVE. p1: previewTunnel.ts
    machineText puts the bar's address back into text for the agent ("Send this page" and the inspector pick now
    name http://localhost:5179/ while the page loads through the tunnel); 2 new tests. p2: the machine name in the
    bar ends in an ellipsis; the full name stays in its hover text. NEXT: M1d debugger.
    RESUME HERE (before, 2026-11-17 05:00): M1c DRIVEN LIVE on the local test master (release test build from
    b3423ff; the window binary rebuilt, the master binary left as it was because the running test master holds it
    and M1c has no Rust change). Scratch dev server E:\qc-scratch\m1c-drive\server.js on 127.0.0.1:5179 prints the
    Host header it gets. PASS: master shown -> m1b-drive Preview -> localhost:5179 loads through a tunnel the
    window opened on 127.0.0.1:33211 (page saw Host localhost:33211); the bar keeps http://localhost:5179/ with the
    machine's dot and name; a relative link stays in the tunnel; This PC's project with the same address loads
    directly (Host localhost:5179, no machine badge); back to the master reuses the same tunnel; the inspector
    opens the tunnelled page and picks an element; "Send this page" reads it. "Open in the browser" not pressed
    (would open the owner's browser). FINDINGS, await owner: p1 the text attached for the agent by "Send this
    page" and the inspector names the tunnel address (http://localhost:33211/), not the master's
    (http://localhost:5179/); the agent works on the master, where 33211 means nothing. Proposed fix: name the
    address from the bar (the master's), front end only. p2 the machine name in the bar is cut off
    ("DESKTOP-R8G3PAK (tes"); proposed: shorter label or the full name in the tooltip only. NEXT: owner's choice on
    p1/p2, then M1d debugger.
    RESUME HERE (before, CHECKPOINT 2026-11-17, fresh conversation): M1c BUILT, NOT YET DRIVEN LIVE. Check free
    RAM before the release test build (5.2 GB free at the checkpoint; stray node/MCP processes were cleared). The preview of a master's project loads
    that master's localhost through port_forward on a free local port (src/lib/previewTunnel.ts plus 4 tests,
    PreviewTab.svelte, api.browser.forward). The address bar keeps the master's address and shows the machine's
    dot and name. The frame, inspector, page text and "Open in the browser" use the tunnelled address. Public
    addresses and This PC's projects load directly. No Rust change. Checks green: svelte-check 0 errors, 444
    front-end tests, prettier. NEXT: drive M1c live (a dev server on the local test master, its project's
    preview, switch back to This PC), then M1d debugger. Details are in docs/MIGRATION-VIEWER.md (M1 preview bullet).
    RESUME HERE (before, CHECKPOINT 2026-11-17, fresh conversation): M1b LEFTOVERS DONE. Read docs/MIGRATION-VIEWER.md M1 (preview/inspector and debugger bullets) first; k1 also fixed (a6a425b). Live install output 55a3f48 (host_run streams output
    lines to a listening caller; window shows the newest four under Installing) plus npm --loglevel=http (without
    a terminal npm printed nothing until its end, so the first drive showed no lines for TypeScript). DRIVEN LIVE
    (release test builds, owner's yes for the three npm downloads): Svelte installed on the local test master from
    Settings, Languages (npm fetch lines streamed through the master, cleared at the end, "Installed by Qanuvi");
    Pyright on This PC from the editor offer on gyp_main.py (lines shown, offer gone, server started, Problems 1);
    TypeScript on This PC from Settings (before the loglevel fix). need_gb measured with a stdio driver (peak
    working set of the server's process tree, 12 files open, 90 s): TypeScript 0.69 GB on a copy of Qanuvi's
    front end (168 TS/JS files, its node_modules) -> 0.7; Svelte 0.61 GB on its 63 .svelte files -> 0.7;
    Pyright 0.31 GB on node-gyp's gyp (58 .py, 25k lines) -> 0.4 kept. Scratch: E:\qc-scratch\needgb-web,
    needgb-py. The local test master service was stopped and now runs the new build. NEXT: M1c preview through
    port_forward, then M1d debugger.
    RESUME HERE (before, 2026-11-17): k1 FIXED (owner's yes; no tray icon in it, the icon stays in M4). On Windows
    master.rs start_service now clears HANDLE_FLAG_INHERIT on this process's own stdin/stdout/stderr before the
    spawn, so the serve service no longer holds the --stdio caller's pipes. New integration test
    tests/master_stdio.rs runs the real qanuvi-master --stdio, closes its input and needs the caller's output to
    end while the service keeps running; it failed with the fix taken out (output never ended) and passes with it
    (1.2 s), then stops its service. Rust verify green: fmt, clippy -D warnings desktop + server build, 437 lib
    tests. Scratch services no longer need stopping by hand for a caller to finish. NEXT: M1b leftovers (need_gb
    for TypeScript/Svelte/Pyright, live install output), then M1c preview through port_forward, M1d.
    RESUME HERE (before, CHECKPOINT 2026-11-17 03:50, fresh conversation): M1b DRIVEN LIVE ON THE VPS, ALL PASS. Node.js put on basevm378 with the
    owner's yes (Debian packages: nodejs 18.20.4, npm 9.2.0; apt, no outside script). Reopened index.php: the bar
    now offered Install -> Intelephense went to /root/.local/share/qanuvi/language-servers/php, run by the VPS's
    `qanuvi-master serve` -> undefined_call() flagged, Problems 1, hover on greet over ssh shows its signature. This
    PC shown: the VPS server ended (none left); basevm378 again: started, no new offer. NEXT: M1b leftovers (need_gb
    for TypeScript/Svelte/Pyright, live install output), k1 (owner), then M1c preview through port_forward, M1d.
    RESUME HERE (before, 2026-11-17 03:30): m1 + m2 FIXED f945170 and DRIVEN LIVE on the local test master (release
    test build 03:05): tools off -> editor status line "Coding tools off on DESKTOP-R8G3PAK (test)", clicking it opens
    Settings, Languages; tools on again -> server back on the already open index.php at once (Problems 1), no reopen.
    vitest 439, svelte-check 0/0. VPS (owner yes): server build cross-built 03:11 (zigbuild -j 2, 9.4 MB), swapped into
    ~/.local/bin (old kept as qanuvi-master.prev, old serve ended with pkill -x). basevm378 shown -> project
    ~/qanuvi-test/m1b-vps added -> index.php: no Install button, the bar says "This server runs on Node.js; install
    Node.js from nodejs.org on that machine first"; its Languages page lists only the VPS's statuses, each missing
    one with its reason (rust-analyzer: rustup). PASS. Not done: a running server on the VPS (it has no Node.js;
    installing it there waits for the owner's yes). Test helper note: in Git Bash put MSYS_NO_PATHCONV=1 before
    typing a Linux path through ui.ps1 (else /root/... becomes C:/Program Files/Git/root/...).
    RESUME HERE (before, 2026-11-17): M1b DRIVEN LIVE on the local test master, all pass (release test build from
    main 3ca3e89; scratch project E:\qc-scratch\m1b-drive with index.php). Master shown -> "+" added the folder on
    that machine -> index.php opened -> install offer for Intelephense "on that machine" -> Install: npm download
    went into the master's data folder (local-master\language-servers\php), server started by `qanuvi-master
    serve`, not by the app -> undefined_call() flagged, Problems 1, hover on greet shows its signature. Switch to
    This PC: the master's server ended (0 processes); back: started again, no new offer. Languages page on the
    master lists that machine's statuses ("rust-analyzer Found on DESKTOP-R8G3PAK (test)"), its own Format on save;
    "Keep coding tools off on DESKTOP-R8G3PAK (test)" stopped its server at once, This PC's switch stayed off.
    Findings for the owner (not changed): m1 with coding tools off nothing on the editor says why (only the
    Settings page); m2 turning tools on again does not re-attach an already open file (same as the per-server
    switch on This PC: the server starts on the next file open or machine switch). Open: VPS drive (needs the
    owner's yes for the new server build there), M1b leftovers (need_gb TypeScript/Svelte/Pyright, live install
    output), k1, then M1c preview through port_forward, M1d debugger.
    n1 (OWNER, 2026-11-17, fix FIRST AFTER THE MIGRATION, on the master): a blocked plan step's "Needs your answer"
    card only said "blocked: it needs you" (seen on step 20 of TINYCODE-FINAL-PLAN in qpfixbase: the reason, an nginx
    rule to add to the server's nginx.conf, was only in the chat). The owner must be able to answer from a phone
    without reading the chat. (a) The question states the whole matter on its own: what failed, why only the owner
    can act, the exact change wanted (the rule, with a copy button), and what each answer does. (b) The answer box is
    a real answer, not "what to do differently": the agent reads it and acts on it. "Insert the block in nginx.conf
    yourself" means it does that (asking for anything it is missing, such as the file's place); "I don't understand"
    brings back a new, more detailed question; an answer that does not settle the need brings a follow-up question
    instead of a blind retry. The question goes on until the need is met, the owner skips the step, or ends the run.
    Try again / Skip / End stay as quick answers. Asks in words, never a restart by itself
    (feedback: chat asks, not buttons).
    RESUME HERE (before, CHECKPOINT 2026-11-17 02:40, fresh conversation): read docs/MIGRATION-VIEWER.md (M0, M1a,
    M1b marked BUILT) and the entry below. Done this session: M0 parity test 3825c57, M1a master pipes a87eb34, M1b
    language servers on a shown master 523d49c, 0.1.24 published from old-infrastructure (handover fix + live chat
    token count), same fix on main 9be8f8e. Open: drive M1b through the app (local test master, then VPS with the
    owner's yes for the new server build); M1b leftovers (need_gb for TypeScript/Svelte/Pyright, live install
    output); finding k1 (a --stdio-started serve service keeps the caller's output pipe; stop scratch services after
    live tests); then M1c preview through port_forward, M1d debugger. Live test helper: ignored Rust test
    lsp::live_runs_a_server_on_a_real_master (env QANUVI_LIVE_MASTER/DATA/PROJECT, output to a file, not a pipe).
    RESUME HERE (before, 2026-11-17 02:30): 0.1.24 PUBLISHED from branch old-infrastructure (782a328, tag v0.1.24;
    owner "publish anyway", release gate skipped). Fix (2271908, cherry-picked to main as 9be8f8e): the chat handover
    keeps off the session a plan run works in, also between its turns (the core's session id goes empty there; on
    0.1.23 the chat handover took Step 6 at 152k, its note went to a session nobody continued and the runner started
    "try 2" from scratch); a step's chat and sessions continuing from it use the run's handover limit, other chats
    the default; the stats line counts a running turn's model calls live (deduped by message id). Rust runner
    untouched. Not driven live with a real run crossing the limit. NEXT: drive M1b in the app, then M1c.
    RESUME HERE (before, 2026-11-17): M1b BUILT (language servers on a shown master; see MIGRATION-VIEWER.md M1b).
    Verify green (prettier, svelte-check, 432 vitest, fmt, clippy -D warnings both builds, 437 Rust tests). Live
    check: ignored test lsp::live_runs_a_server_on_a_real_master against a real `qanuvi-master --stdio` (env
    QANUVI_LIVE_MASTER/DATA/PROJECT): 30 messages in order (one 100 KB), error output, stop on connection end, pass.
    Its service must be stopped after (k1 holds an output pipe). NOT driven through the app window yet. Open in
    M1b: measure need_gb (TypeScript, Svelte, Pyright), live install output. NEXT: drive M1b in the app (local test
    master + VPS, owner's yes for the VPS build), then M1c preview through port_forward.
    RESUME HERE (before, 2026-11-17): M1a BUILT (pipes.rs; master pipe_*/port_*, app port_forward/port_unforward,
    seq-ordered writes, closed with the connection). Rust verify green (fmt, clippy -D warnings desktop + server
    build, 435 tests). Live check against a real `qanuvi-master --stdio` on a scratch data folder: pipe echo with
    out-of-order sends, exit event, HTTP through port_open, closed port refused, left-open pipe ended with the
    connection, all pass. Not yet through the app window (nothing on screen uses the pipes until M1b/M1c).
    FINDING k1 (in 0.1.23 already, not fixed, owner to decide): when `--stdio` starts the background `serve`
    service, the service inherits the caller's stdout pipe on Windows (handle inheritance, though its stdio is
    null), so the caller does not see the end of output when the stdio process exits. NEXT: M1b language servers on
    a shown master (machine questions: which/free memory/install with live output; remove the lsp #root block).
    RESUME HERE (before, 2026-11-17): M0 BUILT. Parity test master.rs
    answers_every_window_command_or_it_is_the_interfaces (app handler list vs master arms; 35 INTERFACE, 14 GAPS with
    phases; fails on a new unlisted command, a closed gap still listed, or a stale name; checked that it fails when a
    gap line is removed). NEXT: M1a master pipes (pipe_start/send/stop, port_forward), then M1b-M1d.
    RESUME HERE (before, CHECKPOINT 2026-11-17, fresh conversation): MIGRATION PLAN AGREED IN FULL by the owner
    (docs/MIGRATION-VIEWER.md, read it first). Owner rule: nothing in Qanuvi's plan or way of working changes, only
    the layer design (screen = container, work in the master); the screen looks the same after. Settled: coding tools
    are the interface's and run on the shown machine through two master pipes; account on the master (Qlopy
    application password in a terminal; qanuvi.com needs a Machine passwords page, no core change); masters update
    themselves (the screen only shows and switches settings); This PC's master keeps running after the screen closes,
    with its tray icon (R4); every old open item mapped to a phase; releases M1 alone, M2+M3, M4-M6, M7-M9 (owner's
    yes each). Size: about 1.5-2x the 24z3 work. NEXT: M0 parity test, then M1a master pipes (pipe_start/send/stop,
    port_forward), M1b language servers, M1c preview, M1d debugger on a shown master. Check free RAM before builds.
    Old infrastructure backup: branch old-infrastructure, tag v0.1.23-old-infrastructure, E:\Backups\qanuvi.
    RESUME HERE (before, 2026-11-17): SPLIT AGREED, plan rewritten in docs/MIGRATION-VIEWER.md (M0-M9). Master =
    any machine; coding tools (language servers, debugger, preview) are the interface's, run on the shown machine through
    two master pipes (run-and-pipe, port forward), optional per machine; account on the master (terminal: Qlopy
    application password, site needs a Machine passwords page, no core change). Open: M8 self-update, This PC master
    after app close, release grouping. Waiting for the owner's go on M0/M1.
    RESUME HERE (before, 2026-11-16 22:30): MIGRATION PLAN DRAFTED in docs/MIGRATION-VIEWER.md (measured: gap is 49
    desktop-only commands, not 57; Inbox and plan runs already on the master; phases M0-M7; five owner decisions).
    Waiting for the owner's review and answers; no code until agreed.
    RESUME HERE (before, 2026-11-16 21:55): 0.1.23 PUBLISHED (owner: "publish anyway"; release rule skipped with
    QANUVI_RELEASE_ANYWAY=1 because Claude Code 2.1.325 was out and 2.1.318 is here). Check before it was short:
    release build starts on This PC, 2/2 machines, a new terminal runs a command; chat, plan step, machine switch
    and Inbox Open into another window NOT checked. Finding j1 (small): a restored terminal tab whose shell ended
    with its master (leftover test master stopped to free the locked qanuvi-master.exe) stays blank with no note
    that the shell has ended. NEXT: (2) below, write the migration plan (app = viewer of masters) before any code.
    OLD INFRASTRUCTURE BACKUP (owner, 2026-11-16 22:05): branch old-infrastructure + tag v0.1.23-old-infrastructure
    (pushed; the branch's OLD-INFRASTRUCTURE.md says how to go on without the migration: i1, i2, Part F, R3-R6, j1);
    data at E:\Backups\qanuvi\old-infrastructure-0.1.23 (installed app's qanuvi.db snapshot, integrity ok,
    attachments, snapshots, backups.json, the 0.1.23 installer + .sig). To go back: switch to that branch, tell the
    agent "skip the migration plan and go on".
    RESUME HERE (before, CHECKPOINT 2026-11-16 21:40, fresh conversation): OWNER DECISION, NEW DIRECTION. The
    master is Qanuvi itself (projects, files, agents, plans, Inbox, memory, settings, its own server list), fully
    usable from a terminal with no display; the app is only the visual container (editors, chat, voice, layout,
    windows) and shows one master at a time. This PC runs its own master and the app shows it by default; a VPS runs
    only a master and is added as a server from any app; masters can connect to masters (nested dropdown under This
    PC's). Nothing on screen is shared with This PC. Today: the app has its own built-in core (desktop.rs, 199
    commands) and masters answer 152; 57 are desktop-only (inbox_*, remote_*, lsp_*, dap_*, notify_*, account_*,
    inspector_*, attachment_*, data_location_*, ...), which is why each screen part was moved one by one.
    ORDER NOW: (1) release 0.1.23 from main as it is (fixes the false usage-limit stop of 0.1.22, plus 98 commits):
    short everyday check of the release build on scratch projects (open project, chat, plan step, terminal, switch
    machine, Inbox Open into another window), then scripts/publish-update.mjs 0.1.23 ONLY with the owner's yes;
    GitHub CI does not block a local release. (2) write the migration plan (app = viewer of masters) before any
    code. NOT DROPPED (owner 2026-11-17: nothing in the plan changes, only the layer design): i1, i2, step 5 Part F, R3-R6, j1 each land in a phase, see docs/MIGRATION-VIEWER.md "Nothing in Qanuvi's plan changes". Since 18:25: Linux CI FIXED
    (275e4f6, terminal test waited for the Windows-only cursor question; green on GitHub), CI posts failing tests
    as public annotations (393e284, dd87db8; read them with the checks API, 60 unsigned requests an hour); one
    Windows "Checks and tests" failure on GitHub at 275e4f6 not explained (full verify passes here); VPS runs the
    20:35 server build with agents_working (.prev kept). Rust 431 pass, vitest 428 pass, clippy clean.
    RESUME HERE (before, CHECKPOINT 2026-11-16 18:25): owner AGREED both findings below, as
    proposed: (i1) a closing window (or the app) keeps its unsaved files as drafts on their machine (B2 park path, in
    onCloseRequested before the window goes) and the Inbox says so; (i2) agent processes start in a job object so the
    --mcp-browser helper ends with its agent. Not built yet. ORDER NOW: build i1 + i2 and drive them live (scratch
    projects only), drive Inbox Open into another window, then step 5 Part F, R3-R5, full check, 0.1.23 (owner's yes to
    publish). GitHub ubuntu failure still open (see below): needs the failing test names, or the owner's yes to run the
    cross-built Linux test binaries on the VPS. VPS server build lacks agents_working (rebuild + swap needs a yes).
    Release test build 18:05 has the closing-order fix (1f6b883).
    RESUME HERE (before, 2026-11-16 18:10): ORDER STEP 4 BUILT (b9d6211, 173fc7f) and driven live on This PC + local
    test master. (a) Working icon: agents_working (desktop core + master, agent::working_count = sessions running or
    waiting, chats and plan steps alike); stores/working.svelte.ts reads every connected machine every 6 s (and when the
    list opens); an older master answers nothing, so no icon for it. Live: a 40-line turn on the local test master
    showed the spinner on its row and on the closed switcher of the other window; gone after the turn. (b) "Open in a
    new window" in a machine's right-click menu (machine-* windows, ?machine=<id|pc>, capability added): the window
    opened on DESKTOP-R8G3PAK (test) with its projects and kept draft; the other window's row shows a window icon;
    picking that machine there brought its window forward and left the picking window as it was; Inbox Open of an item
    of a machine another window shows brings that window forward at the item (emitTo qanuvi-inbox-open, not driven
    yet). (c) Reopen: the main window keeps the machine windows open when it closes (qanuvi.machineWindows); the last
    window ends the app (as built in 15c: closing the main window alone leaves the others running); next start
    brought the machine window back on its machine. FIX in 173fc7f, seen live: the list was wider than the sidebar with the new icons, so dots and
    ticks were cut off; now as wide as the sidebar (collapsed: 220px), long names shorten. Closing order FOUND + FIXED
    (this commit, driven live): a machine window closed by itself came back at the next start (its own close note
    did not leave in time); now the core announces a closed window's machine like its project, and the main window
    keeps the list current on every change. Live: machine window closed first, then main: next start main only;
    VPS window open, main closed first, then it: next start both, VPS window on basevm378.
    FINDINGS (proposed, not fixed): (i1) closing a window or the app drops unsaved text with no question and no draft
    (true before step 4 too; only closing a tab asks): proposal, a closing window keeps its unsaved files as drafts on
    their machine (the B2 park path, done in onCloseRequested before the window goes) and the Inbox says so. (i2) an
    agent turn's --mcp-browser helper (qanuvi-master/qanuvi --mcp-browser) stays running after its agent process
    ends when the app closes (Windows does not end children with their parent): proposal, start agent processes in a
    job object that ends with them. NEXT: Inbox Open into another window (not driven yet), then owner picks i1/i2; then step 5 Part F, R3-R5.
    GitHub Build: ubuntu job fails in "Rust tests one at a time" since 9f6b97f (last green da34ae9); logs need a
    signed-in view, test name unknown; Linux server test binaries cross-built (zigbuild --tests) ready to run on the
    VPS if the owner agrees.
    RESUME HERE (before, 2026-11-16 17:45): h1-h3 DRIVEN LIVE, ALL PASS on This PC + local test master; VPS still
    runs the 16:58 server build. Release test build 17:29, server build cross-built 17:32 (zigbuild -j 2, 9.4 MB, zig
    lives in E:\DevTools\zig\zig-windows-x86_64-0.13.0, put that folder on PATH), not yet swapped onto the VPS.
    Stale leftovers of earlier conversations stopped first (old local test master tree, old conversation process).
    (h1) local test master: unsaved line in step3-proj notes.md, switched to This PC, file moved aside, back: Inbox
    "Unsaved changes to notes.md on DESKTOP-R8G3PAK (test) are kept" tagged DESKTOP-R8G3PAK (test) and kept by that
    machine filter (no This PC items shown). VPS on its older build: the same note lands tagged This PC (fallback,
    never lost), as designed. (h2) VPS picker: opening an empty folder keeps the dialog size; Up and the path box stay
    at the same place, Up goes back, Cancel closes. (h3) last session's six numbered paragraphs now read 4, 5, 6 ... in
    one list. VPS (owner yes): 17:32 server build swapped into ~/.local/bin (old one kept as qanuvi-master.prev,
    pkill -x, window reconnected by itself, 2/2, draft typed back); h1 there: note tagged basevm378 and only under
    that filter. The This PC filter still holds two older test notes (the old-build fallback one and the step-3 one
    from before the fix), test data only. h1-h3 DONE. NEXT: step 4 (working icon + "Open in a new window"), step 5 Part F, R3-R5.
    RESUME HERE (before, CHECKPOINT 2026-11-16 17:30, fresh conversation): h1-h3 FIXED (owner: "fix them"), not
    driven live. (h1) the window puts a note about a master's own work into that master's Inbox: inbox.add(item,
    master) calls the master's new inbox_add (master_inbox.rs: add_new + watchers "add"); an older master without it
    gets the note on This PC instead, never lost. The draft note in +page.svelte passes its machine. (h2) the folder
    picker dialog has a fixed height (min(600px, 100% - 48px)), the list takes the rest, so Up and the path box no
    longer move. (h3) agent markdown: numbered or bullet items with blank lines between them stay one list (numbered
    on), a list keeps its own first number (<ol start="2">). Checks: TS 425 (inbox +1, markdown +1), svelte-check
    0/0, prettier, Rust master tests 18 (inbox_add), clippy, server build check. NEXT: rebuild the release test build
    and server build, drive h1-h3 live (draft note tagged with the master and in its filter, on the local test master
    and the VPS; picker height; a numbered-paragraph answer), then step 4 (working icon + "Open in a new window"),
    step 5 Part F, R3-R5. The 17:00 test window and local test master were still running at this checkpoint (stop
    them before rebuilding, the exe is locked while they run).
    RESUME HERE (before, 2026-11-16 17:15): ORDER STEP 3 DRIVEN LIVE, ALL PASS. Release test build 16:55, server
    build cross-built 16:58 (zigbuild -j 2, 9.3 MB) and swapped into ~/.local/bin on the VPS (owner yes). (f1) local
    test master: "+" opens "Add a project folder on DESKTOP-R8G3PAK (test)" at its home folder, folders only; a typed
    path + Enter goes there; a typed missing path is refused on "Add this folder" ("... cannot be found."); the real
    folder was added on the master only (never in This PC's list) and opened. VPS: "+" opens at /root, a folder click
    goes in ("No folders in here."), Up goes back; Cancel adds nothing. (f2) VPS shows no shell picker (one shell
    only, picker hides at one) and its terminal is /bin/bash; local master keeps "Windows PowerShell". (a) a turn
    started on the local master, switched to This PC at once: This PC's chat untouched, no "working" state; back on
    the master the whole answer is there, Finished. (b) unsaved line in notes.md on the master, switched to This PC,
    file deleted on disk, back: Inbox "Unsaved changes to notes.md on DESKTOP-R8G3PAK (test) are kept" with why and
    the text; file put back, switch away and back: the draft is typed back (Unsaved), no second note. FINDINGS
    (proposed, not fixed): (h1) that draft note is tagged "This PC" in the Inbox though the draft belongs to the
    master (its title names the master); it should carry the master's tag and filter. (h2) the folder picker's height
    follows the folder count, so Up and the path box move after opening a folder (a click aimed at Up hit the
    backdrop and closed it); give the list a fixed height. (h3) chat answers with numbered paragraphs show "1." on
    every item (each blank-line-separated item is its own list and its start number is ignored). Left in place:
    scratch project step3-proj on the local master (session scratchpad), its notes.md draft unsaved. NEXT: owner
    picks which of h1-h3 to fix, then step 4 (working icon + "Open in a new window"), step 5 Part F, R3-R5.
    RESUME HERE (before, 2026-11-16 16:40): ORDER STEP 3 BUILT (not driven live). (f1) Projects "+" on a shown
    server opens Qanuvi's own folder picker on that machine (FolderPicker.svelte, stores/folderPicker.svelte.ts): starts
    at its home folder, path box (Enter goes there; a typed path is checked on the machine before "Add this folder"),
    Up, its folders (click opens one); every read names the machine (callOn), a switch closes it, and a folder picked
    on one machine is never added to another. Master answers dirs_list (projects::list_folders: folders only, sorted,
    full paths, home for ""). A project added from the sidebar on a master is owned at once (remote own()). This PC
    keeps the system picker. (f2) the shell list is read again from the shown machine on each switch (late answers
    dropped). B2 leftovers: (a) a hidden machine's agent turn events never reach the window (agent.handler names the
    machine shown when it was made; its chat is read back on return), and the chat's "working" state is cleared on a
    switch; (b) a draft whose file cannot open again stays kept and now says so once in the Inbox (kind "drafts",
    one key per file, why it did not open, the text to copy from, cut at 4000 characters). Checks: TS 423
    (folderPicker 3, shells 2, draft preview 1), svelte-check 0/0, prettier, Rust projects 9 + master dirs_list,
    clippy, server build check. Noticed meanwhile: the owner's installed 0.1.22 run sat on step 5 after it was done
    (the old window runner read "rate limit ... reached" in the step's own answer as a usage limit; fixed in 9ea5fcd,
    waits for 0.1.23); owner told Pause then Resume. NEXT: drive step 3 live (release test build: "+" on the local
    test master and the VPS, shells on the VPS, a turn running on the master while This PC is shown, a draft whose
    file was deleted while hidden), then step 4 (working icon + "Open in a new window"), step 5 Part F, R3-R5.
    RESUME HERE (before, CHECKPOINT 2026-11-16 16:30, for a fresh conversation): g1 + g2 FIXED (owner: "fix them and
    make checkpoint"), not driven live (text only; the test window still holds an unsaved scratch line in projm's
    README, so it was not rebuilt). g1: the sidebar under a held machine says "Not connected." where it said "No
    projects yet. Use + to add a folder", and shows no empty-chats line. g2: the Servers page's remote access card of
    a held machine shows only "Not connected. Its remote access shows here once it is back." (no boxes, no Turn on).
    Checks: svelte-check 0/0, prettier, TS 417. 24z3 order step 2 (C2 + E) DONE. NEXT (fresh conversation): order
    step 3 = B2 leftovers + f1 (Projects "+" on a shown server adds a folder on that machine, not "this PC") and f2
    (shell picker lists the shown machine's shells, not Windows PowerShell on a Linux server); then step 4 working
    icon + "Open in a new window", step 5 Part F, then R3-R5, full check, 0.1.23 (ask before publishing). Glance at
    g1/g2 at the next release test build.
    RESUME HERE (before, 2026-11-16 16:15): 24z3 PART E DRIVEN LIVE, ALL PASS (owner yes to the three choices: 2 min
    before the wipe, no automatic tries on "not connected", start-up stays on This PC). Release test build 15:55 (old
    local test master stopped with the owner's yes). Local test master shown, its serve process stopped: cover came,
    went by itself on the next try (a fresh master started). Master kept away (its exe renamed for the test, put back
    after): countdown and "Last try: The master program was not found..." on the cover, projects and chats dimmed
    (pixel check 711 -> 354) and out of the accessibility tree, a key typed did not reach the editor; after 2 min the
    opaque "is not connected" screen; Show This PC -> This PC's own screen; picking the master in the switcher while
    away -> its "not connected" screen (nothing of This PC); Reconnect while away -> error stays; Servers... opens
    uncovered; Reconnect once back -> whole screen back. Unsaved README line on the master -> lost -> wiped -> back:
    typed back unsaved. VPS basevm378: the window's ssh ended -> cover -> back by itself in about 2 s. Test note: a
    test line ("typed while lost") went into projm's README while still connected (unsaved, scratch only). FINDINGS,
    proposed, not built: (g1) under the "not connected" screen the empty sidebar says "No projects yet. Use + to add
    a folder" (dimmed, inert) - say nothing, or "Not connected"; (g2) Servers page while a held machine is shown: its
    "remote access" card still offers Turn on (fails) - show it as not connected. NEXT: g1/g2 if the owner agrees,
    then order step 3 (B2 leftovers + f1/f2).
    RESUME HERE (before, 2026-11-16 16:30): 24z3 PART E BUILT (not driven live yet; f3 answered). A shown master
    whose connection ends by itself stays shown: its screen stays as it was, read-only (agent and work panes inert,
    its projects and chats in the sidebar dimmed and inert; the Inbox and the switcher stay usable) under a see-through
    cover "<name>: connection lost, reconnecting…" with the countdown, the last try's error, Try now, Show This PC and
    Servers… (the Servers page is never covered). Its calls keep going to it and fail (forgetMaster keepShown), never
    to This PC. Tries: 2, 4, 8, 15, then every 30 s (RETRY_WAITS); back -> the whole screen is read again (show), the
    unsaved text never left the editor. Not back within 2 min (LOST_FOR) -> wiped to the opaque "<name> is not
    connected" screen (drafts held in the window as in B2, empty sidebar), Reconnect only by hand from then on.
    Picking a master that does not answer opens that same "not connected" screen (the machine before is hidden as when
    switching; the old error line in the switcher menu is gone); picking the shown held machine again tries it now.
    Disconnect or Remove on purpose still shows This PC; the D3 restart flow is unchanged. Switcher row says
    "reconnecting…" while lost. Files: switcher.svelte.ts (held), masters.svelte.ts (#lose, retryNow,
    showUnreachable), machines.ts (showOffline, park sees held), MachineHeld.svelte, +page.svelte, Sidebar.svelte,
    MachineSwitcher.svelte. Tests: TS 417 (masters.lost 6 new, park 3, remote 1), svelte-check 0/0; no Rust change.
    Owner choices to confirm: 2 min before the wipe, no automatic tries on the "not connected" screen, start-up still
    stays on This PC when the remembered machine does not answer. NEXT: drive Part E live (release test build; local
    test master stopped while shown, then started again; then one past 2 min; then pick it while stopped; VPS too),
    then step 3 (B2 leftovers + f1/f2).
    RESUME HERE (before, 2026-11-16 16:00): step 2 started (owner: "go ahead" on the proposed order 1 C2, 2 E, 3 B2
    leftovers + f1/f2, 4 working icon + "Open in a new window", 5 F). Check done first: the window already followed
    every machine's Inbox, merged one list, badges in the switcher, answers to the owning machine; missing were the
    items' machine names, a filter, Open switching machine (bug: it picked the project on the machine shown), dropped
    machines' items vanishing, notifications without the machine. 24z3 PART C2 BUILT (1a5d1cd + layout fix) AND
    DRIVEN LIVE, ALL PASS (release test build 15:58, local test master; test questions put in its DB while stopped):
    each item names its machine on a small line under the title (colour dot; This PC grey), "All machines / This PC /
    each master" filter (Clear all follows it), the master's question answered from the Inbox while basevm378 was
    shown (master DB: answered, no switch), "Open the plan" on the master's item switched basevm378 -> test master
    and opened projm's plan, master stopped -> its items stay greyed "... is not connected" (questions: "Answer it
    once it is back"), nothing to press; reconnect -> the live list replaces them. Removing a master drops its items.
    A master's desktop notification starts with its name ("basevm378: Needs your answer: ...", bundled lines too;
    InboxItem.machine, never stored; Rust test) - not seen live (needs a live add on a master), tests only. An answer
    from a master's Inbox no longer says "(in window desktop)". Tests: TS 407, Rust notify 5 + inbox 11, svelte-check
    0/0, clippy, server build check. NEXT: step 2 part 2 = Part E (not connected screen + Reconnect; overlay
    "connection lost, reconnecting" for a shown machine that drops, f3).
    RESUME HERE (before, 2026-11-16 15:15): 24z3 PART B2 DRIVEN LIVE, ALL PASS (order step 1 done). Release test
    build 14:49 (old local test master stopped with the owner's yes so the build could replace its exe), server build
    cross-built 15:03 (zigbuild -j 2, 9.3 MB) and swapped into ~/.local/bin on the VPS. This PC (projl): README unsaved
    line + ping in a terminal -> local test master -> back: text back unsaved, same tab, the ping process kept running
    while hidden, typing after return works (echo printed), nothing asked. Master (master-proj): same, both ways.
    Terminal panel closed and reopened: same shell, its output, takes input. Master dropped while shown (both its
    processes stopped) with a second unsaved line: window fell back to This PC; picking the master in the switcher
    reconnected it and typed both lines back (Undo gives the saved file, Redo the draft); its terminal tab gone (shell
    ended with the connection, as specified). VPS basevm378 (new scratch project /root/b2-scratch, left in place):
    draft + ping -> This PC 20 s -> back: draft back, ping went on (icmp_seq 6 -> 48), Ctrl+C and echo work. Test
    note: SendKeys Ctrl+C does not reach a Windows console shell (also in a never-switched tab), so stop Windows test
    processes directly. FINDINGS for step 2: (f1) on a shown server, Projects "+" says "This picks a folder on this
    PC" and points to the Servers screen; per-machine screens should add a folder on the shown machine; (f2) the shell
    picker shows "Windows PowerShell" while a Linux server is shown (should list that machine's shells); (f3) a shown
    master that drops jumps to This PC (Part E's "not connected" screen with Reconnect is the planned answer). NEXT:
    order step 2 (check first what of C/C2/E already works and tell the owner, then C2, E, B2 not-done items, F).
    RESUME HERE (before, checkpoint 2026-11-16 09:30): OWNER CHOSE THE AGREED ORDER, no quick 0.1.23. Since 0.1.22
    (2b4dd63) 78 commits: the Rust runner (TS runner deleted), 24z3 A1-A3, D1-D8, B1 (all driven live), B2 + its
    terminal-panel follow-up and the false usage-limit fix 9ea5fcd (tests only, not driven live). Order now:
    (1) drive B2 live (list in the 08:15 note below; also close and reopen the terminal panel: same shell, its
    output back); (2) 24z3 leftovers: Part C2 (one Inbox screen for all machines), Part E (not connected screen with
    Reconnect), the B2 NOT DONE items (hidden machine's agent streams, a draft whose file cannot open), Part F (drive
    live side by side, window memory with several masters); first check what of C/C2/E already works and tell the
    owner; (3) R3 queue, R4 tray mode, R5 drive live; (4) full check, publish 0.1.23 (owner's yes first). Until then
    the installed 0.1.22 can still read a step's own "rate limit ... reached" text as a usage limit: the owner
    presses Pause then Resume.
    RESUME HERE (before, 2026-11-16 08:15): 24z3 PART B2 BUILT (not driven live yet). A machine leaving the screen is
    wiped from the window (machines.ts park(), run at the start of show() and not waited for: every call names the
    machine being hidden, remote.ts callOn): the workspaces are flushed and their projects leave workspace.restored,
    editor.park() empties the editor groups and answers the unsaved files as drafts, kept ON THAT MACHINE (new table
    drafts, migration 12, drafts.rs; drafts_put/list/drop on the desktop core and the master; dropped with a removed
    project). A machine whose connection already ended (masters #dropped) gets its drafts held in the window
    (stores/drafts.ts, in order through one queue) until it is shown again. Terminals: only the shown machine's have
    a screen (terminals.mounted); a hidden machine's are parked: term_detach on its core, the shell goes on, the core
    keeps its last 256 KB of output from a line start (terminal.rs Outlet); on return the screen calls term_attach and
    gets that output (and its end, if it ended meanwhile); answers xterm gives to old output (cursor question) are not
    typed into the shell. OWNER AGREED (08:30) and BUILT: a closed terminal panel or work pane now parks its shells
    the same way (terminals.parkOne; the next screen attaches again), only closing the tab stops a shell (a parked
    tab closed is stopped by terminals.close). A dropped master's terminal tabs go (its shells ended with the connection); its projects open
    fresh ones from their workspace. On return the project's workspace is given back as at start-up, then its drafts
    (applyDraft: typed in, so Undo gives the file's text; a file changed since keeps the draft's fingerprint, so Save
    shows the conflict); agent configuration files with drafts reopen after the switch (project "" + meta). Chats
    already came back from the machine on each switch (agent.loadProject), unchanged. Tests: Rust drafts (3), master
    drafts command, terminal keep_recent + detached-then-attached (ends meanwhile); TS 405 (drafts, terminals,
    machines.park new; switcher mocks); svelte-check 0/0, clippy, server build check. NOT DONE / to look at: running
    agent turn streams of a hidden machine still reach the window (spec: only while shown); a draft whose file cannot
    open again stays kept with no word on screen; window memory with several masters not measured (Part F). NEXT:
    drive B2 live (release test build): unsaved file + terminal running on This PC, switch to the test master and
    back (text and terminal output back, nothing asked), same on the master side, a draft while the master drops,
    then the VPS (server build swap).
    RESUME HERE (before, 2026-11-16 07:50): D8 finding FIXED (ff87cbd: empty Memory text names a project only when
    one is open; driven live on the VPS). 24z3 PART B SPLIT (owner: "go for part B"): B1 = the switcher, B2 = a
    hidden machine wiped from the window and rebuilt on return (editors, chats, terminals per machine; drafts kept on
    the machine), still to build. B1 BUILT a552399 AND DRIVEN LIVE, all pass: MachineSwitcher.svelte replaces the
    "My workspace" placeholder at the top of the sidebar: This PC (always) + each paired master with its colour,
    connection dot (green/amber/red/grey), unread Inbox count per machine and a badge on the closed switcher for the
    others, address on hover only (SSH / relay / test master); a master not connected is connected first, an error
    shows in the menu; "Servers..." opens the Servers page. switcher.svelte.ts keeps the shown machine reactive and
    the pick (qanuvi.shownMachine, main window only); machines.ts pick()/restoreShown(): at start the main window
    shows the machine picked last when it connected, a dropped connection shows This PC but keeps the choice.
    Servers page stopgap "Show This PC" line and eye buttons removed; "This PC" as a name was already refused
    (machineNames RESERVED). 394 TS tests (switcher.test.ts new), svelte-check 0/0, prettier. Driven (release test
    build 07:40): menu lists all three with dots and tick; picking the test master switched the whole screen; window
    closed and started again -> back on DESKTOP-R8G3PAK (test); VPS disconnected on its card (grey dot), picked in
    the switcher -> connected and shown; This PC picked again. Not driven: collapsed sidebar look, the unread badges
    (no unread items in the test data), Part C/E items. NEXT: B2 in a fresh conversation.
    RESUME HERE (before, 2026-11-16 07:20): 24z3 D8 BUILT (3438ed5, owner go) AND DRIVEN LIVE on This PC, the local
    test master and the VPS, all pass. Built: memory column part (migration 11: scope '' -> me, else project);
    Entry::part()/is_account(); This project needs a project, account parts drop it; the disk writes, reads and
    restores account memory only (format 2; project entries in older files are left out); an account entry moved
    into a project gets a new id and the old one a deleted mark (other machines drop it); a new entry from an agent
    is always suggested; memory_suggest takes part (all_projects = me); Memory page: part radios first (project
    picker only for This project, refused without one), kind second, list grouped About me / This project / Tools
    and models, part filter, disk text says what travels. Checks: 419 Rust + 391 TS, clippy (desktop + server)
    clean. Driven (release test build 07:00): both DBs migrated (the D6 and D7 test entries were project-scoped ->
    This project). D6 disk test fact removed from the master's DB by hand (agreed clean-up). Master synced while
    This PC's file was still format 1: the D6 fact did NOT come back, "0 entries on the disk". Master: This project
    + Tools entries saved, grouped; sync "1 entry". This PC: start-up sync "0 entries" (its project fact stayed
    home); About me entry + sync "2 entries", the master's Tools entry arrived as Tools, its project entry did not.
    Saving This project with no project -> "Choose the project this entry is for." Moved About me -> projl: old id
    deleted, new project copy; moved the master's Tools entry -> projl, synced: master "1 deleted here", no project
    copy there. VPS (server build cross-built 07:10, swapped with pkill -x, serve restarted as before, card
    reconnected green): Memory defaults to About me (no projects), About me + Tools saved and grouped, both deleted
    again. Not driven: an agent's memory_suggest live (unit tests only; costs tokens). Small finding: the empty
    text says "for this project" also on a machine with no project. NEXT: Part B (switcher) with (d).
    RESUME HERE (before, 2026-11-16 06:40): OWNER AGREED the Qanuvi ecosystem picture and batch D8 (below), before
    Part B. NEXT: build D8 only when the owner says go; then Part B with (d). The later ideas (learning suggestions,
    research, voice, how the brain gains knowledge, the brain instructing models from memory) are in
    docs/IDEAS-UPCOMING.md "Brain and memory".
    THE PICTURE (owner, 2026-11-16): the BRAIN belongs to the Qanuvi app itself, the same for every user (how to
    decide, research, app abilities such as voice); it gets better only through Qanuvi updates or the central
    qanuvi.com system. MEMORY belongs to the user: account memory (About me, Tools and models) follows the user's
    account to every machine; machine memory (This project) stays on the machine that has the project. The brain
    reads a user's memory to work for that user; the memory never becomes part of the brain. Nothing learned,
    noticed or researched is ever a rule: it arrives as a suggestion until the owner sets it.
    D8 (agreed, not built): memory parts and what the disk shares. (1) Every entry has one of three parts: About me,
    This project, Tools and models (the main choice when saving; today's kinds stay as a second choice). (2) About
    me and Tools and models are account memory: they travel through the memory disk for now (account sync through
    qanuvi.com may replace or join it later). This project entries never leave their machine: the disk writes and
    reads only account memory (replaces the D7 finding's proposals: project entries are not shared at all).
    (3) Today's "every project" entries become About me; project-scoped ones stay This project. (4) Only the owner
    makes an entry active: anything from an agent or from Qanuvi arrives as a suggestion. Drive live on This PC, the
    local test master and the VPS; the old "D6 disk test fact" left on the master's DB from the disk is cleaned up
    (it is This PC's project entry).
    RESUME HERE (before, 2026-11-16 06:10): 24z3 D7 DRIVEN LIVE on the local test master, all pass (release test
    build 05:44). This PC Memory: "1 entry on the disk from this computer" (c); Sync now. Master shown -> Memory page
    loads (no "cannot be done" line); master Sync now -> "read This PC" (b). New entry "D7 master memory test" saved
    into the master's DB only (This PC's DB unchanged), listed; Try it gives the master's slice; the master synced on
    its own after the change ("2 entries"); Edit (title + " edited") saved on the master; Delete (Qanuvi's own dialog)
    marks it deleted on the master and the page shows "No entries yet". Show This PC -> Memory lists This PC's own
    entry only. Driving note: the trash icon and the dialog button are both named "Delete"; ui.ps1 invoke hits the
    icon (closes the dialog), so click the dialog button by position. FINDING (older than D7, not fixed): an entry
    scoped to a project is kept with that machine's project id; the disk carries it to other machines, where no
    project has that id, so it never shows there ("D6 disk test fact" is scoped to This PC's projl: in the master's
    DB but not on its page). Proposal for the owner: map the scope by the project's folder/name when merging, or show
    entries of unknown projects under "every project" with a "from another machine" note. VPS (owner yes, 06:20): D7
    server build (cross-built 05:51, 9.3 MB) swapped into ~/.local/bin with `pkill -x` (ssh -i ~/.ssh/vps_key),
    reconnected from the card -> green. basevm378 shown -> Memory loads; New entry "D7 VPS memory test" (every
    project) listed, Try it gives it, Delete removes it ("No entries yet"). Master left running as before. NEXT: owner
    decides the project-scope fix (discussed 2026-11-16), then Part B with (d).
    RESUME HERE (before, 2026-11-16 05:40): 24z3 D7 BUILT (not driven live yet). A master answers memory_list,
    memory_save, memory_delete, memory_slice, memory_search from its own memory (master.rs, same arguments as the
    desktop; test answers_memory_entries_from_its_own_memory). (b) This PC's disk file is named "This PC"
    (memory_disk.rs writer/named_for; a master keeps its own name; the file still goes by the lasting id). (c) "1
    entry" singular in the memory disk panel. Also: a switch reloads the Memory page and the waiting suggestions
    (machines.ts memory.touch()), and an answer from the machine shown before a switch is dropped (MemoryView load,
    memory.refreshWaiting; test memory.machine.test.ts). Checks: 391 TS tests, master + memory_disk Rust tests,
    clippy clean, server build checks. NEXT: drive live on the local test master (Memory page lists/adds/edits/
    deletes the master's entries, the "D6 disk test fact" it merged shows; This PC's file reads "This PC" on the
    master after a This PC sync; "1 entry"), then the VPS (owner yes for the swap), then Part B with (d).
    RESUME HERE (before, 2026-11-16 05:20): 24z3 D6 DRIVEN LIVE on the local test master, all pass (release test
    build 04:55). Master shown -> Memory: "Only on this machine / A folder", typed path (no Browse), text names
    DESKTOP-R8G3PAK (test); folder + passphrase saved -> "Passphrase kept on DESKTOP-R8G3PAK (test)", its own file
    <lasting id>.qxm in the folder, credential master-memory-disk (This PC's stays memory-disk). Window CLOSED, master
    alone synced on its own at 05:07:41 (start + 90 s + 10 min; memoryDiskLast in its DB). This PC: test entry + same
    folder and passphrase -> its own file, "1 entries ... read DESKTOP-R8G3PAK". Switch to the master -> its own
    status ("2 minutes ago"), not This PC's; master Sync now -> "1 new" from This PC. Remote access with the master
    shown: heading/text name it; local relay (qanuvi-relay 127.0.0.1:47300) -> "Connected to the relay", link saved
    in the master's own relay.json; This PC's panel still off; turned off again after. Master Agents page: its config
    files (home + master-proj), no "cannot be done" line. Clean up now not pressed (logic tested).
    VPS (owner yes): D6 server build (cross-built 05:14, zigbuild -j 2, 9.3 MB) swapped into ~/.local/bin with
    `pkill -x`, reconnected from the card -> green. basevm378 shown: Memory names it; A folder + Save -> "This server
    reads the memory disk's passphrase from its environment (QANUVI_MEMORY_DISK_PASSPHRASE)" (set back to Only on
    this machine, nothing left on the VPS); Agents: Claude Code "Not found", its own config files, no "cannot be done".
    OWNER AGREED ALL FOUR (2026-11-16): (a) new batch D7 = memory entries on a master (memory_list, memory_save,
    memory_search, memory_slice, memory_delete answered by the master), before Part B; (b) This PC writes its machine
    name (or "This PC") into its disk file, not the bare hostname; (c) "1 entry" singular; (d) Part B's switcher
    remembers the shown machine across restarts. CHECKPOINT: NEXT (fresh conversation) build D7 + (b) + (c), drive
    live (local test master + VPS), then Part B with (d).
    FINDINGS (as found): (a) memory entries on a master: memory_list/save/search/slice/delete are
    in no batch (Part A map group 1), so the master's Memory page shows "cannot be done ... (memory_list)" and the
    entry it merged from the disk cannot be seen; proposal: a small batch D7 after D6, before Part B. (b) the disk
    names This PC's file by the bare hostname, so the master reads "from DESKTOP-R8G3PAK (test); read
    DESKTOP-R8G3PAK" (looks like itself); proposal: write This PC's machine name (or "This PC") into its file.
    (c) "1 entries": singular. (d) the window always opens on This PC after a restart, even when a master was
    shown; proposal: leave to Part B (switcher remembers the shown machine). Left behind: test entry "D6 disk test
    fact" (This PC test data + master), disk folder in this session's scratchpad (59246cc1; helper scripts
    ui.ps1, room.ps1, build.sh, lbuild.sh, mdb.js there too).
    RESUME HERE (before, 2026-11-16 04:45): 24z3 D6 BUILT (not driven live yet). Memory disk per machine: each core
    keeps its own passphrase (memory_disk.rs passphrase(): This PC "memory-disk", a desktop master
    "master-memory-disk", a server reads QANUVI_MEMORY_DISK_PASSPHRASE and says so on Save/Forget), reads its own disk
    from its own settings (memoryDisk; a server disk now carries target + identity, written in by the window with
    withServer, also filled at start for disks chosen before D6), syncs ON ITS OWN 90 s after start then every 10 min
    (desktop run() and master start_own; one sync at a time), keeps the last result + problem in its settings
    (memoryDiskLast) for memory_disk_status. The window: no own 10-min timer, asks the shown machine's status every
    minute and on a switch (drops late answers), Sync now / 15 s after a memory change calls the shown machine after
    settings.flushed(), no disk sent. Panel on a master: its own folder (typed path, no Browse), "A folder on a server"
    chosen on This computer for now (the SSH address and key are This PC's), wording names the machine. Remote access
    per machine: relay_* go to the shown master (its own relay link and pairings), This PC still uses its own
    this-computer service; panel heading/text/boxes follow the switch. Agent setup on a master: master answers
    agent_config_list/read/write (its home folder, its projects), agent_configure, agent_clean_up, so the master's
    Agents page loses the "cannot be done on <machine> yet (agent_config_list)" line. FOUND + FIXED on the way: the
    this-computer master service (remote access) runs serve on the app's OWN data folder, so it started This PC's
    start-up list, Claude Code update, OmniRoute watch and daily backups a second time (into the master backup
    folder) and used master-models: key names for This PC. Now the desktop writes qanuvi-app.mark in its data folder;
    a master opened there is This PC (app_data): This PC's key names, start_own and backups left to the desktop core.
    Green: 413 Rust (3 new), clippy 0, server build check, 389 TS (5 new: memoryDisk.machine.test.ts + withServer),
    svelte-check 0/0, prettier. NEXT: drive D6 live (release test build; local test master: folder disk, passphrase,
    sync on its own with no window, status after switch; This PC still syncs; remote access panel on the master;
    master Agents page config files; VPS gets the D6 server build, swap with `pkill -x`), then Part B (switcher).
    RESUME HERE (before, 2026-11-16 05:20): (a) + D5 DRIVEN LIVE, all pass, one bug found and fixed (release test
    build 05:05; VPS got the D5 server build, swapped with `pkill -x`). (a): basevm378 shown -> Claude Code "Not
    found" + install hint, no Windows path; the master on this PC shows its own. D5: This PC's status bar warning
    ("kiro: sign in again (+1)") now comes from its core's omniroute_look; an older master (VPS before the swap) shows
    no warning and no error; the local test master: Connect Qanuvi made a key named Qanuvi in this PC's OmniRoute
    (port 20128), kept as master-models:omniroute ("kept in DESKTOP-R8G3PAK (test)'s credential store"); its
    OmniRoute panel lists that machine's connections; each core's watch filed its own items (This PC's a minute after
    start, the master's on its look). BUG FOUND + FIXED: "Don't warn" saved the muted list and asked for a look at the
    same moment, so the machine could look with the old list (kiro filed although muted). setMuted now looks after
    settings.flushed() (test proves the order; fails without the fix). Driven in the master's DB: Warn again -> kiro
    filed at once; Don't warn -> resolved at once, omniMuted saved there. VPS (D5 build): no Connect Qanuvi, the
    QANUVI_KEY_OMNIROUTE text, no warnings from This PC. Back on This PC: its own warnings again. Not driven: "Open
    OmniRoute's panel" on a remote master (the note; logic tested), the old window list moving once. Left behind:
    the Qanuvi key in this PC's OmniRoute for the local test master (and its master-models:omniroute entry). Green:
    384 TS, svelte-check 0/0, prettier. NEXT: D6 (memory disk + remote access + agent setup on a master).
    RESUME HERE (before, 2026-11-16 04:20): owner: "fix a and continue to d5, keep them in d6".
    (a) FIXED: machines.ts show() empties agent.info and asks the shown machine again (after its settings and Models,
    so a machine with free models and no Claude Code gets Qanuvi's own agent as ITS default); loadInfo drops an answer
    that comes after a switch and takes a missing answer as "not found" (2 new tests in startup.machine.test.ts; the
    first fails without the fix). D6 NOW ALSO HOLDS (owner): the Agent setup group on a master (agent_config_list /
    read / write, agent_configure, agent_clean_up: Part A map group 1), so the master's Agents page loses its
    "cannot be done on <machine> yet (agent_config_list)" line; D6 = memory disk + remote access + agent setup.
    D5 OMNIROUTE BUILT (not driven live yet): a master answers omniroute_connect / connections / catalog / add_key /
    test / remove / sign_in / make_combo for the OmniRoute on ITS machine, with ITS keys (omniroute.rs connect and
    add_key_with shared by both cores; a server refuses connect BEFORE OmniRoute makes a key nobody could keep, naming
    QANUVI_KEY_OMNIROUTE; a server's sign-ins go through OmniRoute's own page, "panel"). The 15f watch moved out of
    the window into each core (omniroute::look; a minute after start, then every 30 minutes, desktop setup and
    Master::start_own), so a machine not shown is watched too; its Inbox items "sign-in:<id>" (same keys and words as
    before) go to that machine's Inbox with the usual notification. New omniroute_look (both cores): looks now, answers
    the connections for the status bar. Muted connections moved from the window's localStorage (qanuvi.omni.muted) to
    the machine's settings (omniMuted; This PC's old list moved once); muting looks again at once. Window: the
    OmniRoute store follows the shown machine (switched(); late answers dropped; status bar warnings are the shown
    machine's: before, This PC's "kiro: sign in again" showed on basevm378), the panel reloads on a switch, "Open
    OmniRoute's panel" / a sign-in's page open in this PC's browser only for This PC or a master on this computer
    (target local), else a note with the address there; key texts name the machine's credential store. Green: 384 TS
    (5 new omniroute.machine.test.ts), svelte-check 0/0, prettier, rustfmt, clippy desktop + server, 409 Rust (3 new),
    server-build master tests 15. NEXT: drive (a) + D5 live (release test build; local test master: Models page,
    OmniRoute panel, watch; VPS with the new server build: no This PC warnings, Connect Qanuvi hidden), then D6.
    RESUME HERE (before, 2026-11-16 03:45): D4 leftovers DRIVEN LIVE (release test build 03:09, Haiku not used).
    PASS: local test master shown -> Settings, Agents shows that machine's own Claude Code versions ("2.1.325 is out
    (this one is 2.1.318)"; Update not pressed: the local master shares This PC's real Claude Code). VPS: server
    build re-cross-built (zigbuild -j 4, 9.1 MB, 2m 49s) and swapped into ~/.local/bin (old serve and --stdio ended
    with `pkill -x`; `pkill -f "qanuvi-master serve"` over ssh matches its own bash line and drops the ssh, exit 255);
    reconnect from the Servers card -> basevm378 green; Start-up on basevm378 names "the Qanuvi master on basevm378",
    no "cannot be done" line (the D4 commands answer). FINDINGS, owner to agree: (a) BUG: with basevm378 shown, the
    Claude Code card still reads This PC's agent info: "Connected · 2.1.318" and the Windows path of claude.EXE,
    though the VPS has no Claude Code (should be "Not found" + install hint). machines.ts show() never asks the agent
    store again (agent.loadInfo; api.agent.info is routed). Proposed fix: `await agent.loadInfo()` in show() after
    settings.loadMachine() (loadInfo may switch that machine's default agent to Qanuvi's own when it has free models
    and no Claude Code: that is the machine's own setting, so it is right), plus a machine test that a switch asks
    agent_info again and drops the old path; then drive This PC -> VPS -> This PC. (b) GAP, not D4: on any master the
    Agents page shows "This cannot be done on <machine> yet (agent_config_list)": the master answers none of Part A
    map's "Agent setup" group (agent_config_list/read/write, agent_configure, agent_clean_up); no 24z3 batch owns it
    yet. Proposal: add it to D6 or a small D7 before Part B. NEXT after (a): D5 OmniRoute.
    RESUME HERE (before, 2026-11-16 03:30): fold FIXED (owner: "yes fix it and continue") and DRIVEN LIVE, all
    pass (release test build, scratch projl/PLAN-L.md, Haiku 5.5, about $0.05). The core's PlanState keeps
    decidedFolded (serde "decidedFolded", from_json reads it, prune keeps it capped to the kept choices; new Rust
    test). Live: 4 choices decided, folded in 1a, stayed folded through 1b, 1z, the run's end and a window restart; a
    new step 1c with one new choice: "Decided for you (5)" opened by itself. Also (owner, screenshot): the agent chip
    "· switching after the edit in hand" wrapped each part into a column in a narrow pane; now the chip is one line
    (model text cut with "…", full text in its tooltip) and the note reads "· switching…" (the long words in its
    tooltip); driven live in a narrow pane. Also (owner): a step that needed them a SECOND time sent no Windows or
    Inbox notification. Read from the DB (qpfixbase TINYCODE step 8): blocked twice on Haiku, asked once 20:45Z,
    answered in the Inbox 20:52Z ("Try again"), nothing ran until the run panel answer 20:58Z; the installed 0.1.22
    (11-14) still runs the OLD window runner. Today's core runner wakes on an Inbox answer and removes the answered
    question, so a second block is a new question with a notification (new scenario test proves it). Guard added: a
    step blocked or not finished after a try clears any earlier question of the step before asking, so inbox add_new
    can never take it for "the same question asked again" (it skips the notification and shows the old answer);
    holds asked again at a run's start keep the owner's first answer. Green: 406 Rust, 377 TS, svelte-check 0/0.
    NEXT: D4 leftovers (Claude Code version on the master's Agents page; VPS with the new server build), then D5.
    RESUME HERE (before, 2026-11-16 02:55): the "Decided for you" fold DRIVEN LIVE, FAILS at a step change
    (release test build 02:45, scratch projk/PLAN-K.md, Haiku 5.5, $0.02, 3 of 3 done, 4/4 tests). The check
    decided 1a (date format) and 1b (slug rules); "Decided for you (2)" opened, folded during 1a, stayed folded
    when the plan reloaded (1/3); when the run moved to 1b it OPENED AGAIN with no new choice. Cause: the core's
    PlanState (src-tauri/src/plan/planstate.rs) has no decidedFolded, so when the runner in the core writes the
    plan state back (plan_data, echoed to the window) the fold is dropped; the TS tests never pass through it.
    Proposed fix (owner to agree): `decided_folded: Option<u32>` in the Rust PlanState (serde rename
    "decidedFolded", default, skipped when None), kept by from_json; a Rust test that load + save keeps it; then
    drive again (fold, step change, handover, restart, a new choice opens it). Then D4 leftovers, then D5.
    RESUME HERE (before, 2026-11-16 02:45, CHECKPOINT for a fresh conversation): "Decided for you" REMEMBERS a fold
    (owner: "yes it should remember"; questions that need the owner are outside it, in "Needs your answer" and the
    Inbox). Before, `open={some unchanged}` was set again on every step change/handover (the list reloads), so a fold
    never lasted. Now the plan state keeps decidedFolded (how many choices it held when folded): planstate.ts
    decidedOpen/foldDecided, RunPanel ontoggle saves it per plan; it stays folded across steps, handovers and
    restarts, and opens again only when a NEW choice is added. Green: 377 TS tests (2 new in planstate.test.ts),
    svelte-check 0/0, prettier. NOT driven live (needs a plan run whose check decides something). NEXT, in order:
    drive that fold live on a scratch plan; D4 leftovers (Claude Code version on the master's Agents page; VPS with
    the new server build: zigbuild, swap into ~/.local/bin); then D5 OmniRoute. R7 (plan notes box) waits for the
    owner to place it in the order.
    RESUME HERE (before, 2026-11-16 02:30): the On box (owner: "your proposal is just fine do it") BUILT AND
    DRIVEN LIVE. Unticking On stops the command now when Qanuvi (or the master) started it; one started by hand is
    left alone (Rust stop only ends a child it owns); ticking On starts it now (startup.setEnabled; new test in
    startup.machine.test.ts). Live with the master shown: off -> the process under the master ended, grey dot, "off",
    no "Answering", gone from the status bar; on -> a new process under the master, "running", back in the status
    bar. Green: 375 TS tests, svelte-check 0/0, prettier; no Rust change. Test lists emptied after. R7 (plan notes
    box) added as a task. Owner asked (not to change yet): why "Decided for you" opens again on each step change.
    Still OPEN for D4: Claude Code version on the master's Agents page, the VPS; then D5.
    RESUME HERE (before, 2026-11-16 02:10): the start-up list bug FIXED (owner: "proposed fix is fine") and
    driven live. parseSettings gives `startup` its own array (as agents/rules/masters); two new tests in
    settings.test.ts (each parse its own list, never growing or shared; turning one off keeps one entry), both
    fail without the fix. Green: 374 TS tests, svelte-check 0/0, prettier; no Rust change. Live (release test build
    02:03): command added on the master, Off saved ONE entry (enabled false) to the master's DB; master ended and
    serve started by hand: the command did NOT start; the window reopened: the Start-up page renders the one card,
    grey, "off"; Remove emptied the master's list; This PC's list stayed []. Seen once, left over from the buggy
    build: at the first launch after the fix the window started the old leaked command from its cached copy of This
    PC's settings (localStorage, applied before the machine's own settings are read); it rewrote the cache from the
    DB and the next launch started nothing. Still OPEN for D4: Claude Code version on the master's Agents page, the
    daily auto-update, the VPS (pre-D4 build there). Off-behaviour question asked to the owner (Off only stops the
    next start; the running one keeps going).
    RESUME HERE (before, 2026-11-16 01:50): 24z3 D4 DRIVEN LIVE on the local test master, PASS with one bug
    found (release test build 01:41, scratch pc-proj/master-proj). Passed: with the master shown, Start-up names
    "the Qanuvi master on DESKTOP-R8G3PAK (test)", "Start it with the Qanuvi master", that machine's Claude Code
    proxy line; This PC's page keeps its own wording and its own (empty) list. A command added there (a node
    listener waiting on 127.0.0.1:18911) runs as a child of the master's serve, "running" / "Answering at" read from
    the master. Window closed: master and command keep running. Master ended (tree) and serve started by hand with no
    window: the command started again by itself. Restart on the item: a new process under the master. Killing the
    master drops the window back to This PC with "The connection ended ... connect again" (expected). NOT driven
    yet: Claude Code version on the master's Agents page, the daily auto-update, the VPS (it still runs the
    pre-D4 build: expect "cannot be done on basevm378 yet"). BUG (old, from 35b1d0d4 Start-up, shown by D4):
    parseSettings copies DEFAULTS shallowly, so `s.startup.push` (settings.svelte.ts ~338) fills the SHARED
    DEFAULTS.startup array; every parse adds the items again. Live: turning "On" off saved TWO entries with the same
    id (one on, one off) to the master's DB, so the master started the command again on its next start; after that
    the Start-up page would not render (keyed each with a duplicate id), and the list can leak from one machine to
    another. Proposed fix (owner to agree): `startup: []` in the object made at line 275 (as agents/rules/masters),
    a test that two parses never share or grow the list, then drive D4 again. Also seen (not D4, owner to decide):
    turning a command Off only stops it starting next time; the running one keeps going and still shows green /
    "Answering" / in the status bar, while the label says "off". Test master's start-up list emptied after.
    RESUME HERE (before, 2026-11-16 01:40): 24z3 D4 BUILT (start-up/Headroom + Claude Code updates on the shown
    machine), NOT driven live. A master answers startup_start/status/restart/stop, startup_headroom,
    claude_settings_base_url, claude_versions, claude_update (master.rs; its own Startup). Each core starts its OWN
    start-up list from its own settings DB with no window needed: a master in serve (Master::start_own), This PC's app
    at launch (desktop.rs setup; the window still applies it when This PC is shown, each command starts once); a
    master stops what it started on stop and restart. Claude Code "keep it up to date by itself" moved out of the
    window into each core (claude_update::auto_update: own settings updates.claudeAuto, never while an agent works;
    20 s after start, then daily), so a machine not shown is kept current too and Part B's remembered machine cannot
    skip This PC. Window: startup and claudeUpdate stores follow the shown machine (switched() in machines.show; a late
    answer from the machine before is dropped; an update on This PC never shows as busy on the VPS's page); Start-up
    page re-asks Headroom and Claude Code's proxy per machine, names the master ("the Qanuvi master on VPS ... keep
    running with no window connected"), shows a machine's error line (an older master: "cannot be done on VPS yet").
    Green: 372 TS tests (3 new, startup.machine.test.ts), svelte-check 0/0, prettier, rustfmt, clippy desktop and
    server, 404 Rust tests (3 new), new tests on the server build too. NEXT: drive D4 live (test build, scratch
    projects: local test master shown -> Start-up adds a command waiting on a port, state from the master, restart
    the master -> it starts again by itself with no window; Claude Code version on the master's Agents page; then the
    VPS as a Linux master), then D5 OmniRoute.
    RESUME HERE (before, 2026-11-16 01:20): the four findings (a)-(d) BUILT AND DRIVEN LIVE, all pass (owner:
    "yes for a, b okay, c okay, d as you proposed"). (a) `local` with an empty name becomes "<computer name> (test)"
    from its first hello (masters #ownName; the "This PC (test)" default in MastersView add() is gone): live, the
    local test master's emptied name became "DESKTOP-R8G3PAK (test)". (b) Dismiss beside the restore note in the
    Servers page's data-folder panel too (DataFolderPanel; "Restored..." and "The backup was not restored..."
    notes, as backups::dismiss_note): live, a planted note on the local master dismissed there, its
    data-location.json emptied. (c) the address beside the name is left out while the name IS that address (no
    name yet): live with an unreachable nobody@127.0.0.9 card (removed after). (d) the card's pencil is now Edit:
    Name, and for SSH the address, program and key file (Save/Cancel); a name-only change never reconnects; a
    change to how it is reached disconnects, saves and connects again; reaching another machine in the list is
    refused ("This is the same computer as X. The change was not saved.") and the old settings reconnect; colour,
    auto-connect and place in the list stay (masters.edit). Live on the VPS: address -> `local` refused with that
    text and basevm378 back on; a wrong program path saved and shown as the card's error; fixed from the card ->
    connected again. Green: 369 TS tests (4 new in masters.names.test.ts), svelte-check 0/0, prettier; no Rust
    change. Owner on order (2026-11-16): keep it: D4, D5, D6, then Part B (switcher). Owner rule: D4+ just follow
    "the shown machine owns its whole screen" (setup wizard, local/free AI, Headroom, any agent, start-up,
    updates); build them without asking that again. NEXT: D4 in a fresh conversation.
    RESUME HERE (before, 2026-11-16 00:50): 24z3 MACHINE NAMES BATCH DRIVEN LIVE, all pass (release test build
    00:30, scratch pc-proj; the local test master and the owner's VPS). Old test master "This PC": red line asking
    to rename; Rename to "VPS TEST" -> 'Another machine is already named "vps test"' (case ignored); "this
    computer" refused; 41 characters -> "The name has 41 characters; at most 40."; renamed "local test", kept its place
    in the list, connect gave it the first unused colour (purple; the VPS had blue). Colour changed to teal on the
    card -> teal chip "local test" in the status bar while it is shown; This PC plain. Adding `local` again ->
    "This is the same computer as local test. It was not added again." (count stays 2/2). Dismiss on the Backups
    page took the restore note away there AND from the Servers page's data-folder panel. VPS: the server build was
    re-cross-built (zigbuild, 9.0 MB) and swapped into ~/.local/bin (the old one from 2026-11-15 had no lasting id,
    so an SSH twin was added silently before the swap: expected, an old master answers no machineId). Removed and
    re-added with an empty Name: the address shown until hello, then "basevm378" (blue). An older twin entry ->
    "This is the same computer as basevm378. Remove one of the two."; a fresh add -> "... It was not added again.";
    <data>/machine-id written on the VPS. Left: the test build's list is "This PC (test)" (local, auto-connect) and
    "basevm378" (VPS, auto-connect); the VPS master is running the new build. FOUND, small, for the owner: (a) an
    empty Name with the address `local` becomes "This PC (test)" (MastersView add(); passes the rules since only
    the exact "This PC" is refused, but it says "This PC" for another machine: maybe "Test master" or
    "<hostname> (test)"); (b) Dismiss is only on the Backups page; the Servers page shows the same note without it;
    (c) before the first hello an empty-name card shows the address twice (as the name and beside it); (d) a card
    cannot change its program path or key file: a wrong path means Remove and add again. NEXT: owner's answer on
    (a)-(d), then D4 (start-up/Headroom + Claude Code updates) in a fresh conversation.
    RESUME HERE (before, 2026-11-16 00:30): 24z3 MACHINE NAMES BATCH (1)-(9) BUILT (not driven live). New
    src/lib/machineNames.ts: nameProblem (trimmed; at most 40 characters; invisible characters refused: control,
    zero-width, direction marks, private/unassigned; taken names refused case-insensitively after NFKC; "This PC"
    and "This computer" refused, the second added because sentences say "this computer"), uniqueName (names
    from a machine itself: cleaned, cut, numbered "basevm378 (2)"), COLORS + pickColor. (1) names exactly as
    typed: placeTitle capitalises only "this computer"; sentences reworded so a name never starts one (backups
    folder line, data folder line, setup guide memory line, router message "This cannot be done on VPS yet",
    restart "No answer from X after the restart"); usage.place/guides.place now the sentence form. (2) an empty
    Name stays empty in settings (the loader no longer fills it with the address); displayName() shows the
    address until the first hello, then the host name is filled in (made unique). (3) Rename on the server card
    (same rules; a card keeps its place in the list: save() replaces in place). A name kept from before the rules
    (the old test master "This PC") shows a red line asking to rename it. (4) refused on add and rename. (5) a
    colour per machine: picked on the first connect (first unused), eight swatches on the card, a coloured chip
    with the name in the status bar while a master is shown (This PC plain); the switcher's part waits for Part
    B. (6) lasting id: machine::lasting_id keeps a uuid in <data>/machine-id (made once; never copied into a
    backup and kept through a restore: SKIP + clear_data), hello answers machineId, the window stores it per
    server; masters.add/pair connect as fresh: the same id as a machine in the list -> "This is the same computer
    as X. It was not added again." and the new entry (and its relay pairing on this device) is taken out; an
    older entry that turns out to be a twin shows "... Remove one of the two." (7) relay pairings: the computer's
    own name made unique, empty -> host name from hello (no "A computer" stored); this device's name checked
    (length, invisible). Rust relay_client::clean_name (40 characters, invisible dropped) for the name sent when
    pairing, the remote access name and a device's name. (8) see nameProblem. (9) "Dismiss" on the restore note:
    backups::dismiss_note (only a restore note; a moved-data note stays), backups_dismiss_note on the desktop and
    the master. (10) the SSH address on hover is the switcher's: waits for Part B (nothing built). Green: 365
    TS tests (new machineNames.test.ts, masters.names.test.ts), svelte-check 0/0, prettier, rustfmt, clippy
    desktop + server, 22 Rust tests on both builds (new: lasting id, clean names; backups test checks the id file
    stays out of a backup, survives a restore, and Dismiss). NEXT: drive it live (release test build, a local
    master and the VPS: add with an empty name, add the same computer twice, rename to a taken name / This PC,
    colours on the card and status bar, Dismiss), then D4.
    RESUME HERE (before, 2026-11-16 00:00): finding (b) DONE (owner: "do what suits better"). A master's default
    backups folder now follows the system: Windows and macOS keep Documents/"Qanuvi master backups" ("(test build)"
    for a test build); Linux and other systems (usually a server without a desktop) use ~/qanuvi-backups
    (~/qanuvi-backups-test-build): in sight, easy to copy off with rsync/scp, no Documents folder made on a server;
    decided by the system only, so it never moves (backups::master_default_for, new test). Green: rustfmt, clippy
    desktop + server, backups + master backup tests on both builds. Driven live on the VPS: "/root/qanuvi-backups
    (the default)"; the two test backups were moved there from /root/Documents (removed) and listed. Seen: the green
    "Restored the backup ..." note still shows after a later plain restart of the master (stays until the next
    restore): by design and the same on the desktop (shared code: booking a restore clears it); proposed to the owner
    as a small "Dismiss" on the note. Finding (a), machine names: OWNER AGREED (2026-11-16), not built yet:
    (1) the server's Name field is shown exactly as typed everywhere (no capitalising; sentences reworded so a name
    never starts one; shownPlaceTitle only for "This computer"); (2) an empty Name becomes the machine's own name
    from hello (hostname, e.g. basevm378), not the SSH address; (3) the Name editable later on the server card;
    (4) refused: a name another server has (case-insensitive) and "This PC". ALSO AGREED (2026-11-16): (5) a
    colour per machine (picked automatically, changeable on its card; shown in the switcher and status bar; This PC
    stays plain) so the shown machine is never mistaken; (6) every added machine gets a lasting unique id: the master
    keeps a permanent id file in its data folder (master.json is only the running service's address/token), hello
    answers it, the window stores it per server; adding a server whose id is already known says "This is the same
    computer as <name>" and does not add it again; (7) the same name rules for relay pairings ("A computer"
    fallback, "This device's name"); (8) names at most about 40 characters, trimmed, invisible characters refused;
    (9) "Dismiss" on the "Restored the backup ..." note; (10) the SSH address (and computer name) on HOVER over a
    machine in the switcher, NOT as text under the name (owner: keeps the switcher small). Build (1)-(10) as one
    batch in a fresh conversation; parts touching the switcher land with Part B if it is not built yet. Then D4.
    RESUME HERE (before, 2026-11-15 23:00): 24z3 D3 DRIVEN LIVE, all pass (release test build 22:56, scratch
    projects pc-proj and master-proj, the "This PC" test master). This PC's Backups page unchanged (its test-build
    default folder, on, its 3 daily backups, Choose…). With the master shown: "Backups: This PC", on by default, the
    folder Documents\"Qanuvi master backups (test build)" (the default), a typed path (no Choose…), no window-settings
    texts. Back up now: "Backed up.", "Made by you · 0.5 MB · 10 files" (no master.json in it). Then "Several at once
    as one" turned off on the master, Restore… on that backup (the box: "applied when This PC starts again"), the
    restore card, Restore… greyed; "Restart This PC now" asked "Nothing is running there now"; Restart: the old serve
    ended, a new `serve --after-pid=<old>` took over, no restart mark left, the window connected again and still
    showed the master; the green note "Restored the backup ... kept as ...-before-restore", a "Before restore" backup
    listed, and the setting back On. A typed scratch folder: the master made it, Back up now put a backup there,
    "Back to the default" brought the default back. Servers page with the master shown: "Where This PC keeps its
    data" is read-only (its --data path, the restore note, "Moving it from here comes later"). Back on This PC: page
    and its "Several at once as one" unchanged. Seen, not new: the box says "Restart This PC?" because the old test
    master is named "This PC" (Part B refuses that name). LINUX (23:20, owner's VPS, Debian 12): the server build
    cross-built with zigbuild (9.0 MB) in ~/.local/bin, server "vps test" with the key file, connected (basevm378 ·
    linux/x86_64 · master 0.1.22). Backups: vps test: on, /root/Documents/Qanuvi master backups (the default, no
    "(test build)" on a server build); Back up now (master.json not copied); the same restore test passed (setting
    off, Restore…, "Restart vps test now", a new `serve --after-pid=<old>` took over, the note, a "Before restore"
    backup, the setting back on); data folder panel read-only (/root/.local/share/qanuvi). The service was stopped
    after; ~/qanuvi-test (12 KB), ~/.local/share/qanuvi (144 KB), ~/Documents (232 KB) and ~/.local/bin/qanuvi-master
    are left on the VPS; the test build's "vps test" server has "Connect when Qanuvi starts" on. FOUND, small, for
    the owner: (a) "Vps test makes the folder..." (shownPlaceTitle capitalises the server's own name); (b) the
    default makes a Documents folder in a headless server's home (fine on a desktop master; a server might rather use
    ~/qanuvi-backups or beside its data). Seen, known: no memory figure in the status bar for a Linux master
    (usage.rs is Windows only so far). NOT driven: a Mac master (owner gets one soon), the daily check (2 min after
    start). NEXT: owner's answer on (a)/(b), then D4 (start-up/Headroom + Claude Code updates) in a fresh
    conversation.
    RESUME HERE (before, 2026-11-15 21:40): 24z3 D3 CHANGED (owner: "no different, if on or off same for master or
    pc"; then yes to "on everywhere"): backups are ON by default on every machine, the same rules everywhere. A
    master's default folder is Documents/"Qanuvi master backups" (test build: "... (test build)"), its own name so a
    master on the same computer as a desktop app never lists or prunes the desktop's backups (two masters with
    different --data on one computer would still share it: not a case now). backups::master_default_dir; master.rs
    passes it everywhere (start-up restore, daily check, its commands). Page: the "off until you choose a folder"
    text is gone, "Back to the default" on every machine, the same-drive hint also when both paths are under / (Linux,
    Mac). The parts below that say "off by default" / "Stop backing up" are replaced by this. Green: prettier,
    svelte-check, 354 TS tests, rustfmt, clippy desktop + server, 398 desktop / 385 server Rust tests (one run had a
    timing failure in plan::live a_run_goes_on_its_own_thread..., untouched code; passed alone and twice in full
    after). NOT driven live. NEXT: drive D3 live (default folder and on for the test master, Back up now, book a
    restore, Restart the master, the note; This PC unchanged; data folder panel read-only), then D4.
    BEFORE (2026-11-15 21:10): 24z3 D3 BUILT (backups + data folder on the shown machine). Owner
    agreed the proposal; on question (a) a master's backups are OFF by default until a folder is chosen on it (the
    owner's sentence ended at "it should ..."; read as "it should ask for a folder", the page says so; confirm);
    (b) a master's data folder is only shown, moving it comes later. backups.rs: the default folder is now a
    parameter (desktop: default_dir, a master: none); shared set/info/now_backup/book_restore/remove_one used by both
    desktop.rs and master.rs; "Back up now" with no folder says "Choose a backups folder on this machine first."; a
    booked restore with no folder is refused at start ("not restored", nothing changed); master.json and the restart
    mark are never copied into a backup. master.rs answers backups_info/_set/_now/_restore/_cancel_restore/_remove,
    data_location_info (fixed: true, its --data / QANUVI_MASTER_DATA text), master_busy (open agent sessions, plan
    runs) and master_restart; Master keeps its own data folder (paths keeps only the first a process sets, which
    made a test flaky). A master applies a booked restore in Master::open before its database opens, and runs the
    daily check (2 min after start, then hourly) in serve(). Restart: writes a "master-restarting" mark, starts a new
    `serve --after-pid=<pid>` (it waits for the old one to end), answers, then ends (agents, terminals stopped,
    service file removed); ensure_service waits for the coming service while the mark is under 30 s old instead of
    starting a second; the new service removes the mark. Window: Backups page and the data folder panel reload on a
    machine switch (late answers dropped); on a master the header says "Backups: <name>", no Choose… picker (a full
    path typed on that machine), "Stop backing up" instead of "Back to the default", no "window settings" texts,
    "Restart <name> now" asks with what stops there, then masters.restart() lets the connection go, connects again
    (4 tries) and shows the machine again; the restore note shows on the Backups page. Green: prettier,
    svelte-check, 354 TS tests, rustfmt, clippy desktop + server, 398 desktop / 385 server Rust tests (new: a
    machine without a default folder, a master's backup commands). NOT driven live. NEXT: drive D3 live (test build,
    the "This PC" test master: no folder text, choose a scratch folder, Back up now, book a restore, Restart the
    master, the note and the data from before; This PC's page unchanged; data folder panel read-only), then D4.
    RESUME HERE (before, 2026-11-15 20:00): 24z3 D2 DRIVEN LIVE, all pass (release test build 19:35, scratch
    projects pc-proj and master-proj, the "This PC" test master, $0, nothing installed). For the master only, a test
    wrapper took the place of qanuvi-master.exe and started the real one with PATH, LOCALAPPDATA, APPDATA,
    ProgramFiles and ProgramW6432 pointing at empty folders and a stand-in winget.exe that installs nothing and ends
    with exit code 1 (Windows rebuilds ProgramFiles from ProgramW6432 for a 64-bit child, so both are needed). The
    real master was put back after (same bytes). Status bar: $4.19 on This PC, $0.28 and the master's memory (11 MB)
    with the master shown, This PC's again after switching back. Wizard on the master: step 1 shows the master's
    machine line; step 2 "Ollama ... is not installed yet" (This PC has it, so the master answered); Install asks with
    the exact winget line (--scope user, no permission line); Install runs the stand-in in a terminal inside the
    wizard, "[The install has ended.]", "Ollama was not installed (exit code 1)". Step 4 "OmniRoute needs Node.js";
    Install Node.js asks with the winget line and "Windows asks for permission on This PC's own screen: someone there
    has to click Yes"; Cancel starts nothing. Notifications: "Several at once as one" turned off on the master stayed
    on for This PC and stayed off on the master after switching back and forth (put back on after). NOT driven: a
    real notify_item using the master's settings (needs an Inbox item on the master), a password typed into the
    install terminal, a Linux or Mac master. FOUND, small, waiting for the owner: (a) the status bar's month-cost
    tooltip still says "Agent cost this month (all sessions on this computer)" while a master is shown; (b) the
    wizard's outcome line ("Ollama was not installed (exit code 1) ...") stays after "Close the terminal" and on the
    next steps (closeInstall clears installing only, not outcome). UPDATE 20:10: owner said fix both and drive
    them; FIXED and DRIVEN LIVE (release test build 19:56, same wrapper set-up, real master put back after, same
    bytes). (a) StatusBar keeps the shown machine's name with each month-cost answer: "all sessions on this
    computer" on This PC, the master's name with the master shown. (b) "Close the terminal" clears the outcome line;
    moving to another step clears it only when no install terminal is shown (with the terminal still open the
    result stays with it, checked on step 3). Green: prettier, svelte-check, 354 TS tests (Rust unchanged). NEXT:
    D3 (backups + data folder) in a fresh conversation.
    RESUME HERE (before, 2026-11-15 19:30): 24z3 D2 BUILT (machine/setup + usage + notifications on the shown
    machine). Owner agreed: the status bar shows only the shown machine; every install runs the SAME way on every
    machine and system (This PC, desktop masters, servers): the wizard asks first with the exact command (and, when
    Windows will show its permission box on that machine's own screen, says someone there clicks Yes; never a UAC
    bypass), then runs it in a terminal on that machine shown inside the wizard (a sudo or Homebrew password is typed
    there by the owner), then says installed / not installed (exit code) and looks for the tools again. Plans
    (local_servers::install_plan): Windows winget, Ollama --scope user (no admin box), Node.js machine-wide (admin
    box), OmniRoute npm through cmd; Mac brew install ollama; Linux Ollama's script through sh; Node.js on Mac/Linux
    and no winget/brew/curl: "link:<url>" (a button on This PC, plain text on a master). terminal.rs: spawn_program
    runs one program and tells its real exit code once (a watcher thread; Windows keeps the pseudo-console open
    after the program ends). master.rs answers machine_info, machine_room, setup_tools, setup_install_plan,
    setup_install_start (its terminal closes with the connection), qanuvi_usage. setup_install gone. Usage: cleared
    at a switch, late answers dropped, older master shows nothing, tooltip names the machine. Notifications per
    machine: notify_settings window-only and only ever This PC's; a master's settings are read when the Inbox
    follows it (kept current while it is shown) and sent with each notify_item. guides.machine read again per
    machine; D1 leftovers fixed (Models subtitle, Ollama's name and help line, guide and wizard say the machine's
    name). Node.js's Program Files folder added to the usual places (a new install is not on Qanuvi's old PATH).
    Green: 354 TS tests (3 new, setup.machine.test.ts), svelte-check, prettier, rustfmt, clippy desktop + server,
    397 desktop / 383 server Rust tests (new: install plans per system, master answers, a program's exit code in a
    real pseudo-terminal). NOT driven live. NEXT: drive D2 live (test build, scratch master: wizard on the master,
    an install that fails harmlessly, status bar switch, a master's notification setting), then D3.
    RESUME HERE (before, 2026-11-15 18:10): 24z3 D1 DRIVEN LIVE, all pass (release test build 17:58, scratch
    projects pc-proj and master-proj, the "This PC" test master, $0, no model run). This PC: OpenRouter key kept
    (the owner's real one, not touched). Master shown: OpenRouter "Save key" (no key: not This PC's); Find models on
    Ollama answered from the master, 2 models tagged with the master's name; a test key saved on the master went to
    master-models:openrouter only ("Key kept in This PC's credential store", the master's name), This PC's
    models:openrouter and models:omniroute unchanged; switched to This PC (its key and "this computer" texts back)
    and to the master again (its own key back); Forget key removed master-models:openrouter only. Server build
    (--no-default-features master swapped in, desktop master put back after): every provider shows "No key. This
    server reads it from its environment: set QANUVI_KEY_<ID> ..."; no key fields, no Connect Qanuvi. FOUND, left
    for later batches (not D1's commands): still "this computer" in the Models page's subtitle, the Ollama provider
    name "Ollama (this computer)", the Ollama checkbox's help line, and the guide's "This computer: 15.3 GB memory"
    (machine_info, D2). NEXT: D2 (machine/setup + usage + notifications), in a fresh conversation.
    BEFORE (2026-11-15 15:40): 24z3 D1 BUILT (Models + keys on the shown machine; owner agreed the
    proposal and both questions: a desktop master keeps its own key names, brain records per machine). New
    model_keys.rs (one place for both builds): credential store first, then QANUVI_KEY_<PROVIDER>; a server build
    cannot save or forget a key and says where to set it; a master's store entries are master-models:<id>, so a
    master on the same computer never shares This PC's models:<id>. Agents and plan runs on a master now read keys
    the same way (before: environment only, so a key saved on a desktop master did nothing). llm::probe shared.
    master.rs answers model_key_store (new, also on the desktop), model_key_has/_set, models_list, model_probe,
    model_key_check, local_server_status/_start, ollama_pull (streamed), brain_records (the master's data folder).
    Window: machines.show() calls brain.switched() (drops found/probes/notes, reads the machine's keys and records;
    late answers from the machine before are ignored); an older master says "VPS cannot do this yet" instead of
    "no key"; a server shows "Key set in VPS's environment" or the QANUVI_KEY_ text, no key field and no Connect
    Qanuvi; texts say the machine's name instead of "this computer" (remote.ts shownPlace). Green: 351 TS tests (6
    new, brain.machine.test.ts), svelte-check, prettier, rustfmt, clippy (desktop and server), 393 Rust tests, new
    tests on the server build too. NOT driven live. NEXT: drive D1 live (test build, scratch master: Find models and
    a key on a desktop master; server build swapped in for the environment text), then D2.
    RESUME HERE (before, 2026-11-15 14:30): 24z3 A2 + A3 DRIVEN LIVE, all pass (release test build 14:06, scratch
    projects pc-proj and master-proj, "This PC" test master, $0). A2: with the master connected, This PC's sidebar
    stays This PC's only; Show lists only the master's two projects, title and month cost follow it. A3 part 1:
    Minimap differs per machine; turned on for the master and off for This PC, each kept its own; the master's value
    survived a restart (its DB). A3 part 2, desktop master: store text, a DEMO_TOKEN test secret added, listed, removed
    (gone from the credential store). Server master (--no-default-features build swapped in next to the test app for
    the check, desktop master put back after): environment text with QANUVI_SECRET_DB_PASSWORD, DEMO listed from
    QANUVI_SECRET_DEMO, no Add or Remove. FOUND for Part B: the old test master is NAMED "This PC", so the Servers
    page says "The sidebar shows This PC" and the tooltip "On This PC: ..." while the master is shown; Part B should
    refuse "This PC" as a server's name (and rename an existing one). Owner agreed 2026-11-15 the batch order for the
    Settings groups (map group 2), one per conversation: D1 Models + keys, D2 machine/setup + usage + notifications,
    D3 backups + data folder, D4 start-up/Headroom + Claude Code updates, D5 OmniRoute, D6 memory disk + remote
    access (fixes the KNOWN note below). NEXT: D1.
    RESUME HERE (before, 2026-11-15 night): 24z3 A3 SECOND PART BUILT (project secrets on a master). Owner chose
    2026-11-15: a server keeps secrets in its ENVIRONMENT for now (no key file). secrets.rs: kind() is "store"
    (desktop, credential store) or "environment" (server build); a server reads QANUVI_SECRET_<NAME> and gives
    <NAME> to every project's agents and test command; adding/removing there answers how to set it (systemd unit,
    restart). master.rs answers secrets_store / secrets_names / secrets_set / secrets_remove (only for its own
    projects); new secrets_store in the desktop core. Project Settings shows the server's text, hides Add and Remove
    there, and shows a master's error instead of an empty list. Green: 345 TS tests, svelte-check, prettier, rustfmt,
    clippy (desktop and server builds), 389 Rust tests, server-build secrets tests; NOT driven live. Model keys
    (model_key_*) stay for the Models batch (server: QANUVI_KEY_<PROVIDER>, as now). NEXT (fresh conversation): the
    Settings groups in batches (map group 2), and drive A2 + A3 live on a test build with a scratch master.
    RESUME HERE (before, 2026-11-15 evening): 24z3 A3 FIRST PART BUILT (settings on the machine). Each machine's DB
    has a settings table (migration 10, settings.rs: one JSON value per top-level key, "masters" and "account"
    refused, they stay with the window); settings_get / settings_set in the desktop core and in master.rs.
    settings.svelte.ts reads and writes the shown machine's (only changed keys, sent in order to the machine the
    values came from; a change made before the first read is kept and sent after it); This PC's first read moves
    the old localStorage copy into its DB once; localStorage stays as This PC's start-up copy plus the window keys.
    An older master without settings_get: the Settings page says "VPS cannot do this yet" and shows defaults, never
    This PC's. New stores/machines.ts show(machine): remote.ts, the project list and Settings follow it (the Servers
    screen's stopgap buttons and a dropped connection call it; Part B's switcher will, and deletes the stopgap
    buttons). Owner confirmed 2026-11-15: those buttons are placeholders only. Verify parts green: 345 TS tests,
    svelte-check, prettier, rustfmt, clippy, 387 Rust tests, server-only build compiles; NOT driven live.
    KNOWN, for Part D's Settings batches: remoteAccess and memoryDisk values switch with the machine but their
    stores still act on This PC; two windows on This PC do not see each other's changes until reopened (as before).
    NEXT: A3 second part, keys (secrets_*) on the master; then the Settings groups in batches; drive A2 + A3 live on
    a test build with a scratch master.
    RESUME HERE (before, 2026-11-15 afternoon): 24z3 A2 BUILT (the sidebar lists one machine). projects.svelte.ts
    lists only the shown machine's projects (projects_list goes to it through the router), tags a master's with
    master, keeps This PC's ids so a computer paired with itself does not list them twice, and keeps each machine's
    last project (qanuvi.currentProject for This PC, qanuvi.currentProject.<id> for a master). projects.show(machine)
    puts a machine on screen and lists it; a late older list is dropped. projects.addRemote is gone: connecting still
    lists the master's projects (so their ids route there) but only counts them (masters.projectCount, the Servers
    screen); a project window for a master's project shows that master; the shown master's connection ending shows
    This PC again. Adding with the folder picker while a master is shown says to use the Servers screen. STOPGAP
    until Part B's switcher: the Servers screen has "Show in the sidebar" per connected server and "Show This PC".
    338 TS tests (8 new in projects.test.ts), svelte-check and prettier clean; NOT driven live yet (needs a test
    build with a master connected). NEXT: drive A2 live on a test build with a scratch master, then A3 (settings
    on the machine) in a fresh conversation.
    RESUME HERE (before, 2026-11-15 13:00): 24z3 A1 BUILT (the shown machine). Owner agreed both map questions
    (Qlopy sign-in stays with the window: it will decide which machines a device such as a phone connects to, a topic
    for another day; each machine keeps its own memory disk sync; lsp/dap on a master later, said on screen until
    then) and the order A1-A4. Owner restated: a complete separate screen per machine, only the Inbox shared.
    A1: remote.ts has showMachine/shownMachine; a call with no id goes to the shown machine (This PC by default), an
    id still goes to the machine that made it; WINDOW_ONLY lists the window/install commands that never leave This
    PC; a master's "does not know the command" reply becomes NotOnMachine ("VPS cannot do this yet (cmd).", name
    from masters.svelte.ts), never retried on This PC; 13 Settings/setup files moved from invoke to the router. No
    behaviour change yet: nothing calls showMachine until Part B's switcher. 330 TS tests (11 routing), svelte-check
    and prettier clean; not driven live (nothing to see until B). Refusing an id of another machine waits for A2
    (today's mixed sidebar still opens master projects). NEXT: A2 (sidebar lists only the shown machine's projects;
    projects.addRemote goes) in a fresh conversation.
    RESUME HERE (before, 2026-11-15 afternoon): 24z3 owner read-through done, one change: Part C2 (one Inbox
    screen for all machines, stored per machine, badges, answer without switching). PART A STEP 1 MAP DONE (read
    only): docs/design/24z3-part-a-map.md. Routing today follows ids, so id-less calls (projects_list, projects_add,
    term_shells, plan_run_active) always hit This PC; masters.svelte.ts mixes master projects into the sidebar; ~90
    window commands have no master answer (lsp, dap, memory, settings, keys, models...); Settings live in window
    localStorage. NEXT: owner answers the map's two questions (account + memory disk, lsp/dap on a master) and agrees
    the order A1-A4; then A1 (the shown machine in remote.ts) in a fresh conversation. Nothing built.
    RESUME HERE (before, 2026-11-15): NEW DESIGN MILESTONE 24z3 (machines, one at a time through a switcher) comes
    BEFORE R3 (owner agreed). DESIGN COMPLETE: parts A-F plus the owner's decisions (whole screen and Settings are
    the shown machine's, This PC never hidden, hidden machines wiped and rebuilt, drafts, connection overlay, two
    screens, a window per machine, working icon) are all in item 24z3. NEXT (fresh conversation): owner's final
    read-through of 24z3, then Part A step 1, a READ-ONLY map: which stores/components call invoke directly, which
    already pick a master, which commands a master does not answer yet; show the map to the owner before any code.
    R3's proposal is noted under R3, to discuss again after 24z3. Nothing built, nothing published.
    RESUME HERE (before, 2026-11-15 11:00): TYPESCRIPT RUNNER LEFTOVERS DELETED (first item of the agreed order).
    Every run function the window no longer calls is gone from src/lib/plan and core/decider.ts (packet.ts and
    setaside's logic whole; nextModel, stepPrompt, replan/replaceStep, closings, handover, notes, logs, finishing
    prompts and more), with their tests; what stays is what the window shows with (run types, totals, savings, step
    ranges, Plan view state, routing, planner, housekeeping, holdask's reply, finishing estimate). Tests that mixed a
    live and a deleted function keep their live part. The comparison check keeps the recorded cases of deleted
    functions in compare.json untouched (all 457 kept, only reordered), so the Rust port is still held to them; Rust
    plan::compare passes. 323 TS tests pass (83 went with deleted functions), svelte-check clean, prettier clean.
    Not run: full Rust suite and clippy (no Rust code changed). NEXT: R3 (queue, migration 10), then R4, R5, full
    check, publish 0.1.23 (owner's yes first).
    RESUME HERE (before, 2026-11-15 10:15): R2e fixes DRIVEN LIVE (see 24z "R2e fixes driven live"): (a), (c), (d),
    (g), (h), (i) and the planner Model lines work in the test build; (b) and (e) not reached (no long reason, no try
    2). Found and fixed (owner's yes): a step whose Needs name a later step of the same run was skipped instead of
    waiting for it (scenario first, driven live); the Inbox called a paused run "stopped"; the sidebar's Sessions list
    had been squeezed to nothing by the scrolling project list (8ddd58f); now projects take two thirds and sessions
    one third of the free height (owner's choice). Left as is: pausing right after a step's edits shows "0 of N done"
    until Resume (cosmetic). The test build started its "This PC" test master again (qanuvi-master.exe from
    target/release, still running; builds use --bin qanuvi until it is stopped from the Servers page). Nothing
    published. ORDER (owner agreed 2026-11-15 10:30): delete what is left of the TypeScript runner, then R3 (queue),
    R4 (tray mode, visible icon), R5 (drive live), then a full check and publish 0.1.23 (owner's yes first). The
    master accounts milestone comes after that: qanuvi.com has no sign-in yet (domain only), so it is written out in
    parts and its Qanuvi side built against a local stand-in sign-in server; the live check waits for the site.
    The 0.1.22 release (2b4dd63) is the last TypeScript-runner version. The test master was stopped with the owner's
    yes (10:25); full test build rebuilt 10:32 (all four binaries, nothing started). Account site (owner, 10:40):
    while R3-R4 are built, the owner puts up a plain Qlopy site, either a local copy (must be http://localhost or
    http://127.0.0.1, subfolder fine) or live at https://qlopy.top (owner decides which). Test builds point at it
    with QANUVI_ACCOUNT_SITE (and QANUVI_ACCOUNT_CLIENT); the site registers app qanuvi-desktop, redirect
    http://127.0.0.1:47213/callback, scope users:read; endpoints oauth/authorize, v1/oauth/token, v1/oauth/revoke,
    v1/users/me under /api/qlopy. Local: the whole sign-in is checked here with a test account kept in the site's
    seed or example config. qlopy.top: public live site, so the owner signs in in the browser and Qanuvi's side is
    checked after; throwaway test account only. Then the accounts milestone is built against it. Scripts + scratch project (proj, PLAN-R2F.md) in the scratchpad of session
    98d99277.
    RESUME HERE (before, 2026-11-15 09:10, checkpoint): planner Model lines committed (a1ca399; 406 TS + 188 Rust
    plan tests, clippy clean). Owner's new logo v13 (apex corner mark) everywhere (20c416b: app icon, generated
    icons, title-bar logo, favicon; v12 kept in docs/design; exe icon checked by pixels). Test build rebuilt in full
    (target/release, --no-bundle, test data folder dev.qanuvi.desktop.source); the old "This PC" test master was
    stopped with the owner's yes, so start it again from the Servers page when driving master paths. Nothing
    pushed or published. NEXT (fresh conversation): drive R2e fixes (a)-(i) and the planner Model lines live in
    this test build on a scratch project, then delete what is left of the TypeScript runner, then write out the
    master accounts milestone in parts for the owner's review. Helper scripts (room.ps1, rone/rall/rplan/clip.sh,
    ui.ps1, loadmon.ps1, logo.ps1, exeicon.ps1) are in the scratchpad of session fc920636.
    RESUME HERE (before, 2026-11-15, after the R2e fixes): the owner's decisions on R2e (a)-(i) are BUILT with tests
    (see 24z "R2e FIXES DONE"); not driven live, nothing published. Owner (2026-11-15): the planner's "- Model:"
    lines move to plan data too: done (new plans and replanned steps keep the planner's model per step in the plan
    state, the file gets the steps without them). NEXT: drive the fixes live in a test build on a
    scratch project (check free RAM first; Rust parts one job at a time), then delete what is left of the
    TypeScript runner, then write out the master accounts milestone in parts for the owner's review.
    RESUME HERE (before, 2026-11-15, after R2e part 2): R2e PART 2 DONE (see 24z "R2e part 2 DONE"): the master
    paths driven live with a "This PC" master (target local) on a scratch project, one run (3 steps, about $0.22 plus
    the pre-run check): pre-run check, a step held ("Wait for me") answered in this window's Inbox and the run went on
    on the master, disconnecting while a step worked (the run finished on the master), reconnecting (finished run and
    finishing pass offer shown). Fixed: the Plan view of a master project found no task list (the master lacked
    files_markdown and other Plan view commands), and a master project's plan data went to this computer. Two
    findings for the owner (proposals in the 24z entry): the master chose models from its own (empty) run history, and
    the pre-run check's "run 1b after 1c" is not acted on. Nothing published. NEXT: the owner's choice on the two
    findings and the small ones from part 1 (a)-(e), then delete what is left of the TypeScript runner.
    RESUME HERE (before, 2026-11-15, after R2e part 1): R2e PART 1 DONE (see 24z "R2e part 1 DONE"): the Rust runner
    driven live on a scratch project in a test build (4 runs, about $0.60): start, pre-run check, held step answered
    in the Inbox, step chat note / question / rule / change (answered "as a new step"), Pause and Resume, model
    switch, finishing pass, closing and reopening the window. Three bugs found and fixed (scenario or test first):
    the window never saw the step at work, a run cut off by closing Qanuvi reopened as still running, and Plan view
    spinners on steps already done. 183 plan tests + master tests pass, clippy (desktop and master-only), fmt,
    prettier, svelte-check clean. Nothing published. NEXT: R2e part 2 in a fresh conversation: the master paths with
    a "This PC" master (Servers, target "local"; a run on the master project, its held step in this window's Inbox,
    the window following the run), then the open small findings below (owner's choice), then delete what is left of
    the TypeScript runner. Scratch project + scripts (u.sh/ui.ps1 with combos/pick, guard, rplan.sh, clip.sh,
    build.sh) in the scratchpad of session e93ec9c3.
    RESUME HERE (before, 2026-11-15, after R2d part 3): R2d PART 3 DONE (see 24z "R2d part 3 DONE"): the master's
    Inbox commands and stream (master_inbox.rs), the window's Inbox showing each connected master's items and
    sending answers to the machine that has them, one desktop notification per new master question (notify_item),
    and, found on the way, the window now follows a master's run over plan_run_watch (part 2 showed nothing for a
    run on a master). 374 Rust tests (1 old ignored) and 400 TS tests pass; clippy (desktop and master-only), fmt,
    prettier and svelte-check clean. Nothing published, not driven live. Noted, not done: plan data is not routed to
    masters (the Plan view of a master project lacks the master's held-step list). NEXT: R2e, drive it live on a
    scratch project in a test build (list in the entry below), with a "This PC" master for the master paths.
    RESUME HERE (before, 2026-11-15, after R2d part 2): R2d PART 2 DONE (see 24z "R2d part 2 DONE"): the window runs
    no runner of its own any more. stores/runner.svelte.ts is a thin store (same exports) over the plan_run_*
    commands and the "qanuvi-run" / "qanuvi-run-agent" events; a running step's chat goes to plan_run_message (a
    private detail to the new plan_run_tell, never sorted by a model); runner lines are kept in the step session's
    stored events; standing owner rules (a "rule" kind) go into FACTS.md and the run's notes. 373 Rust tests and
    396 TS tests pass, clippy (desktop and master-only), fmt, prettier and svelte-check clean. Nothing published,
    NOT driven live yet. NEXT: R2d part 3 (the master's Inbox commands, so a master run's held steps show on the
    desktop), then R2e: drive it live on a scratch project in a test build (start, step chat note/rule/question/
    change/stop, Pause and Resume, model switch, a held step answered in the Inbox, the finishing pass, closing and
    reopening the window while a run goes on). Still open, small: replaced steps get their own colour (24z).
    RESUME HERE (before, 2026-11-15, after R2d part 1): R2d PART 1 DONE (see 24z "R2d part 1 DONE"): the core side of
    the wiring (plan/live.rs run handles and finishing pass, desktop_runs.rs commands and events, master_runs.rs
    commands and run stream). 371 Rust tests pass, clippy and fmt clean, nothing published. NEXT: R2d part 2, the
    window: a thin RunnerStore with the same exports over plan_run_* and "qanuvi-run"/"qanuvi-run-agent", the step
    chat sending to plan_run_message, the runner's notices kept in the session's lines, standing owner rules; in a
    fresh conversation. Still open, small: replaced steps get their own colour (24z). Map runner.svelte.ts's coupling
    with the agent store (onPlanChange, notice, planSessionId) first.
    RESUME HERE (before, 2026-11-15, after R2c part 2): R2c PART 2 DONE (see 24z "R2c part 2 DONE"): owner messages
    in step chats in the core (sorted, passed on, change asked with a safe default). 365 Rust tests pass, clippy and fmt
    clean, nothing published. No live check yet: the window does not call the Rust runner until R2d, and the live
    drive on a scratch project is R2e. Agreed with the owner 2026-11-15 and written in: 24z2 (tidy code and write
    scope, after R2) and "R2d also: standing owner rules". NEXT: R2d wiring (commands, the qanuvi-run event, a thin
    RunnerStore, the step chat sending to the runner, the master's commands and run stream), in a fresh conversation.
    24z2 also has safe code (sensitive paths, checks on every diff, release gate) and agent extras (part 11), all
    written to suit any project and language, never one owner's stack. Owner's own qCommerce cleanup list (not
    Qanuvi work): tinycode.md in the amirularham root.
    Test runs: `cargo test -j 1` with a `timeout`, output to a file, "running for over 60 seconds" kept visible (a hung
    test once looked like a slow build for 45 minutes). Check free RAM first (scratchpad room.ps1).
    RESUME HERE (before, 2026-11-15, after R2c part 1): R2c PART 1 DONE (see 24z "R2c part 1 DONE"): the run loop
    in the core (plan/run.rs) on RunEnv, all 10 ported scenarios un-ignored and passing, 5 new ones; 350 Rust tests
    pass, clippy clean; TypeScript unchanged, nothing published. NEXT: R2c part 2, owner messages in step chats (24z
    "Owner messages in step chats": scenario tests first, then the decider's sorting in the Rust runner), in a fresh
    conversation; then R2d wiring. Check free RAM before builds and run the Rust parts on their own (guard.ps1).
    RESUME HERE (before, 2026-11-15 checkpoint): 0.1.22 PUBLISHED (owner: "publish anyway", release rule skipped):
    PLAN-F stuck-again fixes (reviews search the plan's folder and check an empty diff against the Done line; replans
    rewrite Needs lines; Headroom "<<ccr:" placeholders give way to the agent's text), the "read my message now"
    stopgap (a quiet agent is told to go on, no more "paused by the owner"), R2b's process-tree stop. PLAN-F was fixed
    by hand (97a-2, 97a-6, 97a-7 [x]); the owner updates to 0.1.22 and reruns it from 97a-3. Owner agreed the design
    for messages typed into a runner-opened step chat (24z "Owner messages in step chats"): build it into the Rust
    runner. Also agreed: replaced steps get their own colour (24z "Replaced steps get their own colour"), and
    Settings + Collapse sit on one line in the open sidebar (done, ffdc9a2, unpublished). PLAN-F runs on 0.1.22 from
    97a-3 (5/13 done at the checkpoint); watch its first review on the fixed prompt. GitHub CI: Linux failed every build since 07d8e38 (#85-#104; Windows and macOS green): installed() in
    desktop_notify.rs was unused on Linux and clippy -D warnings stopped verify; fixed in 7ed071f, check the next Linux
    build (if still red, read the Linux job log for the next cause). NEXT: R2c (the run loop on RunEnv, owner messages included, un-ignore the 10 scenarios) in a fresh
    conversation; check free RAM before builds and run verify in parts (guard.ps1).
    RESUME HERE (before, 2026-11-14, later night): R2b DONE (see 24z "R2b DONE"): the agent turn and its fix
    rounds in Rust (plan/turn.rs) and the process-tree stop. NEXT: R2c (the run loop on RunEnv: step order, attempts
    through turn::attempt, reviews, replans, holds through the Inbox, closing steps, limit waits; un-ignore the 10
    scenarios) in a fresh conversation; check free RAM before builds and run verify in parts (guard.ps1).
    RESUME HERE (before, 2026-11-14 night): R2a DONE (see 24z "R2a DONE"). PLAN-F reran on 0.1.21: 97a-1 done, the
    rest skipped by the Needs-label bug (R2 findings); the plan file was fixed by hand (Needs point at 97a-1...97a-4,
    old skip lines removed) and the owner reruns it. NEXT: R2b (the agent turn and its fix rounds on RunEnv, plus
    the process-tree stop agreed today) in a
    fresh conversation; check free RAM before builds and run verify in parts.
    RESUME HERE (before, 2026-11-14 late): R2 PRE-FIX DONE (see 24z "R2 PRE-FIX"): reviews from Qanuvi's own
    snapshots (git optional per project), empty-diff rule, free replan numbers, replaced steps never held, skip-twice
    fix, and the standing runner scenarios (10) + snapshot layout tests. Driven live in a test build, then PUBLISHED
    0.1.21 on the dev channel (owner: "test then publish anyway"; QANUVI_RELEASE_ANYWAY=1). PLAN-F cleaned (colliding
    steps removed, 97a open again with its Done line); the owner stops the old waiting run, updates to 0.1.21 and
    reruns from 97a. NEXT: R2a in a fresh conversation (split and decisions in 24z R2).
    RESUME HERE (before, 2026-11-14 21:15): R1 DONE (part 2b: preflight, packet, planstate, housekeeping, planner,
    guidance in Rust; see 24z "R1 part 2b"); comparison check 429 cases, all match. NEXT: R2 (the runner in Rust) in
    a fresh conversation: map stores/runner.svelte.ts onto the plan module first, then split R2 into parts.
    RESUME HERE (before, 2026-11-14 20:30): R1 PART 2a DONE (see 24z "R1 part 2a"): tasklist, routing, runner and
    finishing in Rust, 209 comparison cases all match. NEXT: R1 part 2b in a fresh conversation (preflight, packet,
    planstate, housekeeping, guidance, planner), then R2 onwards continuously.
    RESUME HERE (before, 2026-11-14 19:30): R1 PART 1 DONE (see 24z "R1 part 1"): src-tauri/src/plan/ with decider,
    limits, holdask, prompts, picks, costs, setaside, js helpers, and the TypeScript/Rust comparison check (88 cases,
    all match). NEXT: R1 part 2 in a fresh conversation (tasklist first, then routing, preflight, packet, planstate,
    housekeeping, finishing, guidance, planner, plan/runner.ts), then R2 onwards continuously.
    RESUME HERE (before, 2026-11-14, after the 24z split decision): the owner chose the RUST CORE for 24z step A (more
    stable than an embedded JavaScript engine: one language and runtime, no promise bridge, no garbage collector in
    long unattended runs; the TypeScript runner is deleted afterwards either way). Order: R1 port the pure logic
    with its tests, R2 the runner in Rust, R3 the queue, R4 tray mode (step B moved in; owner: a visible tray icon,
    nothing hidden from the user), R5 drive live (scratch project, then the VPS master). See item 24z "Step A plan".
    Each part in its own fresh conversation, checkpoint after each. NEXT: R1 in a fresh conversation (start with
    decider.ts and the smallest plan/*.ts files; port each test file with its code; compare the TypeScript and Rust
    results on the same inputs before anything TypeScript is deleted). Discuss-before-changes is satisfied for R1-R5
    as described; anything beyond it needs the owner's yes. Owner (2026-11-14): build R1 onwards continuously, built
    by hand in conversations (NOT as a Qanuvi plan run); ask only when truly needed (VPS use, downloads, design
    changes). 24y records R1-R5 as the benchmark the plan runner must reach later.
    RESUME HERE (before, 2026-11-14 17:06): 0.1.20 PUBLISHED on the dev channel (owner: "publish anyway"; release
    rule skipped with QANUVI_RELEASE_ANYWAY=1 because Claude Code here is 2.1.318 and 2.1.325 is out: the owner runs
    "claude update" before the next release). 0.1.20 has everything since 0.1.19: plan logins in step chats and masked
    storage, Inbox and desktop notifications (24u part 4), debugging for Node and PHP (15i part 3), split editor and
    minimap (15i part 4), the new logo, the small fixes. Nothing unpublished.
    NEXT (owner chose, 2026-11-14): 24z STEP A, the headless runner and queue (see item 24z: move the plan runner's
    decisions and the decider out of the window into the core, so runs go on with the window closed and on a master
    over SSH; a queue of tasks and whole plans; the window, phone and Inbox show and answer the same queue). Owner rule:
    discuss before changes. First conversation: read 24z, map what stores/runner.svelte.ts and core/decider.ts do
    today (which parts are pure plan/*.ts functions, which touch the window), propose the split (Rust core vs the
    pure TypeScript run by the core) and the parts of step A, and get the owner's yes before any code. Live tests on
    scratch projects only. The brain-training plan and extensions are ideas for later in docs/IDEAS-UPCOMING.md, NOT
    development items; other core items after 24z: 24q, Milestone 1 checks, 10 Stage 1 review.
    CHECKPOINT 2026-11-14 (after 15i part 4): nothing open from 15i. NEXT: ask the owner what comes next (candidates:
    publish 0.1.20 when they say so; 15i LATER Phpactor; 24q agreed, not built; 24v/24w LATER; Linux/macOS checks of
    Milestone 1). Test scripts (ui.ps1 with a drag action, loadmon.ps1, crop.ps1) and the scratch project splitproj are
    in the scratchpad of session 69585bb6.
    UPDATE 2026-11-14 (before): 15i part 3 SESSION B BUILT AND DRIVEN LIVE (bc0b0ae + the two fixes after it,
    unpublished, verify 364 + 172): PHP debugging through PHP Debug v1.40.2 (owner's yes for the download), Xdebug
    file per run, Debugging in Settings > Languages. The (a)-(e) fixes were driven in the same test build: all work.
    See 15i "Built (part 3, session B)" and "Driven live (part 3, session B)". The install offer's doubled name is
    shortened (owner's yes; not driven live, text only). NEXT (owner: "do as order"): 15i part 4, split editor and
    minimap, in a fresh conversation. Owner: do NOT publish 0.1.20 yet.
    UPDATE 2026-11-14 (before): the owner agreed the fixes for findings (a)-(e); they are BUILT (4abc7de, unpublished,
    verify 358 + 169, NOT driven live). Xdebug 3.5.3 (8.2 NTS VS16 x64, SHA256 checked against xdebug.org) is in
    E:/DevTools/xdebug, owner's yes; php.ini NOT changed (Wnmp's PHP serves the live www folder), so it loads per run
    with php -d zend_extension=... NEXT: session B (PHP / Xdebug) in a fresh conversation; drive the (a)-(e) fixes in
    the same test build. Owner: do NOT publish 0.1.20 yet.
    UPDATE 2026-11-14 (before): 15i part 3 SESSION A DRIVEN LIVE: works; five small findings (a)-(e) in 15i
    "Driven live (part 3, session A)", waiting for the owner. js-debug is installed in the test data folder. NEXT:
    the owner decides on the findings, then session B (PHP / Xdebug; Xdebug install needs the owner's yes).
    Owner: do NOT publish 0.1.20 yet.
    UPDATE 2026-11-14 (before): 15i part 3 SESSION A BUILT (848dad2, unpublished, NOT driven live; verify green
    358 + 169, Rust tests need -j 1 when memory is short). See 15i "Built (part 3, session A)". NEXT: drive it live
    on a scratch Node project with a test build (installing js-debug downloads about 10 MB from GitHub: ask the owner
    first), then session B (PHP / Xdebug). Owner: do NOT publish 0.1.20 yet.
    UPDATE 2026-11-14 (before): 24s part 3 driven + both findings fixed (ccfcb00). 15i part 2 DRIVEN LIVE: works,
    five findings (a)-(e) in 15i "Driven live (2026-11-14)", ALL FIXED and driven live (unpublished). Phpactor
    added to 15i as LATER; 24k2 Free Claude Code added as LATER. NEXT: build 15i part 3 (debugging, PHP / Xdebug
    and Node first). Owner: do NOT publish 0.1.20 yet.
    UPDATE 2026-11-14 (later): fixed in 6afe00c (store module in secrets.rs, stub without desktop; Linux
    qanuvi-master cross-builds). Owner: do NOT publish 0.1.20 yet. 24t driven live, fine. NEXT: drive 24s part 3
    (starred chat at 40k+ reopens fresh from its note), then 15i part 2, then build 15i part 3. Earlier next:
    ask the owner about publishing 0.1.20 (unpublished: 779bf51, session A f523130, session B 07d8e38). After that: drive
    24t, 24s part 3, 15i part 2, then 15i part 3. Earlier text of this checkpoint:
    (session A) 24u part 4 session A DRIVEN LIVE on a
    scratch project (test build, Haiku 5.5, $0.01), no fixes needed. PLAN-I: 1a set to "Wait for me" in the check,
    the run waited ("Needs your answer (1)", 1b waits as the same step number), the Inbox showed the question with
    Go ahead / Let Qanuvi decide / Skip / Open the plan; answered "Go ahead" + a note from the Inbox: the DB row went
    answered (answered_by main), the run panel card closed at once, 1a ran with the note (math.js deleted), 1b ran,
    run finished 2/2 and a "finished" info item came in. PLAN-J: the check held 1a by itself ("needs you"); Skip in
    the run panel closed the Inbox question ("You answered: Skip this step"), run ended 0/1, 1 skipped, file kept.
    Not checked live: a second window, the import of old items, closing at the next run start. NEXT: session B
    (owner agreed 2026-11-14 the order: probe first, then build): desktop notifications on Windows, macOS and Linux;
    first a small probe of show + click per system (tauri-plugin-notification on desktop may not report clicks:
    Windows toast activated event, Linux notify-rust action, macOS weakest); fallback when a click cannot be read:
    the notification still shows and a click brings Qanuvi to the front with the Inbox open. Windows is checked live
    here, macOS and Linux by build and code review only (say so). PROBE DONE 2026-11-14 (throwaway crate, not in
    the repo): Windows with tauri-winrt-notification 0.7: the toast shows (owner saw it); the default banner hides in
    seconds and reports "dismissed, timed out" (reason 2) then sits in the notification centre, so use
    Duration::Long (about 25 s) and do not treat that dismissal as an answer; a click on the banner while the sender
    runs reaches on_activated (owner clicked, probe printed "activated"); a click on the notification-centre copy
    after the time-out did NOT reach the process (sent under PowerShell's app id, so Windows routes it there).
    Build plan from it: send as Qanuvi's own app id (the NSIS Start-menu shortcut) and give the toast a protocol
    launch "qanuvi://inbox/<id>" so banner, notification-centre and closed-app clicks all come in through the deep
    link (single instance forwards it); check that with an installed build. Linux: notify-rust 4 with
    wait_for_action cross-builds with zigbuild (zig is at E:/DevTools/zig/zig-windows-x86_64-0.13.0); click not
    tested (no desktop on the VPS; Zorin laptop needed). macOS: notify-rust uses mac-notification-sys, clicks only by
    waiting on the response; not buildable here, code review only. Do Not Disturb is the user's choice; the Inbox
    still holds every item. Earlier text of this checkpoint: 24u part 4
    session A BUILT (f523130, verify green 338 + 162): Inbox table (migration 8, src-tauri/src/inbox.rs, "qanuvi-inbox" event to every
    window), held steps become questions with forms (src/lib/plan/holdask.ts: Go ahead / Let Qanuvi decide / Skip;
    logins answered in the run panel only), the first answer anywhere closes it and reaches the running run
    (runner #fromInbox), run panel answers close it too, questions of a plan close at the next run start when the hold
    is gone, old localStorage items imported once. FIRST: drive it live on a scratch project (fresh plan file, a hold
    step; answer it from the Inbox; check the run goes on and the run panel and Inbox agree). THEN session B: desktop
    notifications on Windows, macOS and Linux (owner, 2026-11-14: not Windows only; tauri-plugin-notification shows
    them everywhere, check whether desktop click events need notify-rust per system), Settings > Notifications
    ("Desktop notification", bundled, quiet hours), click opens InboxQuestion. Older checkpoint (2026-11-14 late): PUBLISHED 0.1.19 (24u parts 1-3, 24x stopped-step
    chat is a chat and resumes only on yes/continue, End the run here = needs-you, chat keeps its place when
    scrolled up, first-decision-lost fix, Team label). Unpublished since 0.1.19: 779bf51 (plan logins reach
    handed-over step chats; stored requests mask private details; migration 7 masks old copies; a login typed during
    a step stored as a placeholder; handover part not driven live) and the 24z design notes. NEXT: 24u part 4
    (central Inbox in the database with answer forms, Windows notifications), see 24u below; design it to be the
    answer channel of the later headless queue (24z step A). Publishing needs the owner's yes. Test plans: the
    scratch project's plan state is kept per plan file (localStorage), so use a fresh plan file name per test. Each part in a short session; drive it live on a scratch project only. After 24u:
    drive 24t, 24s part 3 and 15i part 2 live (Pick models, Revert / Rewind / Redo and the waiting-message hold were
    driven 2026-11-13), then 15i part 3 (debugging, PHP / Xdebug and Node first). Publish only with the owner's yes. Older open notes: drive 24j, 24o, 24n
    part 6, the stale-file fix and the model switch live on the scratch plan (never on the owner's folders). Owner's
    idea asked about (not yet agreed as a step): "run 5 to 8 of PLAN-X" typed in the chat starts that range of the
    plan (the runner already takes ranges; needs a parser and a plan lookup, then a prefilled Run panel or a start
    with the plan's kept settings).
  - Next, in the order agreed (owner, 2026-11-12: "do it as you order"): (1) 24n parts 1, 2 and 4 as one step: step
    cards with IN/OUT, test-browser steps with screenshots, a timeline with grouping, folded thinking and live to-do
    lists (about 2-3 sessions); (2) 24n part 5, the message box (drop files, @ mentions, right-click image paste); (3)
    24n part 3, edits as diffs in the chat; (4) 24j plan housekeeping; (5) 24o finishing pass; (6) 24n part 6 (edit
    and resend, rewind, search) as its own step; (7) 15i language servers and debugging. Later: 24k OmniRoute (only
    when the owner asks), 1e Task Manager grouping, 15e2 terminal list, the custom installer. Each part starts in a
    new conversation.
- [ ] **24u. Runs that keep going, asks that are hard to miss (owner, 2026-11-13; agreed 2026-11-13)**: the owner
  left PLAN-E running and came back 8 hours later: 95a finished at 10:54, then the run ended "needs-you" before 95b
  and sat for over 6 hours. The check before the run sets every risky step to "Wait for me" by default and turns
  every unanswered question into a hold; one hold ends the whole run (runner `waitsBefore` then `break`), and the
  message says "you asked to see it first" although nobody chose it. A run restarted later starts with no holds and
  no answers. Owner: the run must be self-sufficient and stop only when absolutely nothing can be done without them.
  - Part 1, set aside, not stop: a held or failed step is set aside and the run goes on with the steps that do not
    need it. Dependence from the plan first (Needs lines, same step number, files from MAP.md and the step text),
    then one short decider call ("does 72a depend on 95b?") logged in Decisions. Dependent steps are set aside with
    "waits for 95b". A section's z step does what it can and writes what is still open. When only set-aside steps
    are left, the run does not end: "Waiting for you (N answers)", nothing spent, and it carries on by itself when
    answered; "Finish without them" ends it. Answers given while it runs are picked up after each step. Final
    summary: done, decided for you, set aside, waiting for answers.
    - Built 2026-11-13 (driven live 2026-11-13, see the checkpoint): step states "held" (a hold, blocked, or not finished by any model) and
      "set-aside" (waits for a held step); src/lib/plan/setaside.ts reads dependence from the plan (Needs line, same
      step number such as 95a/95b, a file both name or the held step changed) and asks the decider once for the other
      open steps (logged as set-aside decisions; free models answer when the paid agent cannot; no answer: only the
      plan's word holds). MAP.md is not read for this (step text and changed files only). Closing "z" steps are never
      set aside and are told what is. The run panel lists "Needs your answer" with a reply box, Go ahead / Try again
      (the model ladder starts again) and Skip this step (its waiting steps skip too); answers given while the run
      goes on are picked up after the step in hand. When only set-aside steps are left the run waits ("Waiting for you
      (N answers)", one Inbox item per run), with Finish without them; Pause and Stop work while it waits. The old
      "stop at 3 steps needing it" rule and the "Only where a decision is needed" stop choice are gone (saved
      settings with it read as Never). Not done here: answers kept after a restart (part 2); "decided for you" (part
      3).
  - Part 2, kept per plan: answers, holds and "decided for you" choices are stored per plan, so a restarted run
    keeps them. Hold wording says who set it ("you asked" / "Qanuvi's default").
    - Built 2026-11-14 (verify green, 317 + 156): src/lib/plan/planstate.ts keeps per plan (localStorage, next to
      the plan notes) the check's holds with who set them (a risky step left on its default and a question left
      open are "Qanuvi's default"; Wait chosen by hand is "you asked to see it first") and the owner's Go ahead /
      Skip answers from the Needs-your-answer card. A run (with or without the check) holds kept steps again, runs
      answered ones with the answer, keeps skips skipped; entries of steps no longer open go at the next start; a
      later check does not hold an answered step again. Check answers already carry over as plan notes (FACTS.md).
      "Decided for you" choices come with part 3. Driven live on the scratch plan ($0.08): run 1 left the 1a
      question open (held, "Qanuvi's default: a question was left open (...)"), Finish without them; run 2 without
      the check held 1a again at once with the same wording; answered "underscore", stopped before 1a worked; run 3
      ran 1a with underscore without asking, 1b after it, 2 of 2 done.
    - Finding: 1z was done in run 1, so run 3 did not run it again after 1a/1b (README listed only add). Fixed
      (owner: "yes", 2026-11-14), also cheaper: the plan state keeps "closing step ran while X was set aside"
      (closingRan / markLate / closingsDue); a closing step runs again ONCE per section, when nothing it waits for is
      still pending in the run, or once when the run ends (Finish without them included); never while the run
      waits for answers. A closing step done in an earlier run is added to the run that finishes its steps (runner
      `#afterDone` / `#runClosings`, replacing `#reopenClosing`); a waited-for step done by hand counts as late. The
      closing step's set-aside note now says mark it done for what it did, not blocked (live, a 1z marked itself
      [!] over slugify without it). Verify green (319 + 156). Driven live on PLAN-C ($0.1 or so): run 1 held 1a, 1z
      did the add line and was done, Finish without them; run 2 without the check held 1a ("Qanuvi's default"),
      answer "dash", 1a, 1b, then 1z added back and run once (2 attempts in its log over both runs), README names
      slugify, "3 of 3 done". Cost of the re-run: one short closing session (about 11k in, 49k cached, 5k out; $0.005
      on Haiku); before the batching it ran once per late step.
  - Part 3, sorted asks: the check sorts each risk and question into (a) decides itself: a sensible default that
    can be changed later (a setting, not hard-coded), logged "Decided for you: X, because Y. Change?"; (b) goes ahead
    with extra checks: the paid decider reviews the diff and tests must pass before the step counts as done; (c)
    truly needs you: secrets, actions on outside accounts or anything leaving this computer. Risk is judged by where
    the step runs (money on a test database is not real money). Each item comes with the recommended answer chosen,
    plus "Accept all recommended"; holds only where the owner chose one or (c).
    - Built 2026-11-14 (verify green 328 + 156, not driven live): the check's JSON gives each question and risky step
      a sort ("decide" / "checks" / "you"), a recommended answer and a because; unsorted risky steps and questions
      with no recommended answer count as "you" (as before). plan/preflight.ts `settleCheck` turns the form into
      holds, the owner's notes, decided-for-you choices (an untouched recommended answer of a decide/checks item) and
      extra-check steps. The Run panel fills recommended answers in (questions that need you stay empty, the
      recommendation as placeholder), shows a sort tag and the because per item, risky steps choose Go ahead / Go
      ahead with extra checks / Wait for me (starting on the sort's choice), and "Accept all recommended" fills
      everything in as the owner's own answers. Decided choices go into the plan notes (FACTS.md) worded "keep it
      in one place, not hard-coded", into the run's Decisions (kind "decided") at every run, and into the plan
      state (planstate `decided`, `keepDecided`, `changeDecided`); the run panel lists them under "Decided for
      you ... Change?": a change is kept, sent to later steps as an owner note, and for a step already done the
      panel says to open it again. Extra-check steps (options.extraChecks, kept per plan as `checks`) always run
      the full tests, are told so in their prompt, and their "done" goes through the decider's diff review also on
      the paid agent (before only free models' work was reviewed).
    - Driven live 2026-11-14 on the scratch project (PLAN-F, Haiku 5.5, about $0.03): the check sorted 1a date
      format and 1b empty-email rule "Qanuvi decides", 1b migration of local test data "goes ahead with extra checks",
      1c push (no remote) "needs you"; the run did 1a (format kept as settings at the top of util.js, as the decided
      note asks), 1b (the decider's review accepted it), 1z, held 1c and waited; "Decided for you (3)" listed with
      Change; the Team label read "no free models yet". Found and fixed: (a) the first decision of EVERY run was lost
      (`(run.decisions ??= []).push` pushes into the plain array, not the reactive copy; same for deciderCalls, so
      the first decider call's cost was missing from run totals); (b) Change saved an unchanged answer as a change
      (Save now ignores and is disabled for the same answer). Both fixes verified by tests, not driven again.
  - Part 4, one notification centre: the Inbox moves into Qanuvi's database (Rust side), seen by every window, the
    runner and later the phone. Types: for your information, needs an answer, problem. Answer items carry their form
    (options, text, "Let Qanuvi decide", "Skip this step"); the first answer anywhere closes it everywhere. Settings,
    Notifications: per type, Inbox (always), Desktop notification, phone; bundled ("5 things need you"); quiet
    hours. Desktop notifications on Windows, macOS and Linux (not Windows only, owner 2026-11-14). Clicking a notification brings Qanuvi forward and opens the same answer window used in the
    Inbox. Secrets are answered in the main app only. The phone is a delivery target in the design and settings;
    the phone app itself (through the Milestone 20 relay) is its own later step.
- [ ] **24x. Writing in a stopped step's chat is a chat, not a restart (owner, 2026-11-14; agreed 2026-11-14)**: before,
  any message in the session of a step the last run stopped at restarted the run, with the owner's words buried about
  17,000 characters into the step request (no bubble of their own; images dropped and left in the box; every message
  also became a plan note). Owner: the run goes on only when they say so; no buttons in the chat, the agent simply asks
  "should I resume, yes or no?". Plan runs must stay smart on their own for people who are away (hold only what needs
  them, do the rest), and step-by-step watching with messages while it works stays as built.
  - Built 2026-11-14 (verify green 331 + 156): runner `stoppedAt` (a step of the last run, the run not done and not
    running); the message goes to the chat as usual (bubble, images, box cleared) with a hidden note (guidance
    `stoppedChatNote`, STOPPED_MARK): answer first, do what is asked here, keep the plan's mark true, no other steps,
    and end with "Should I resume the plan run from step X? (yes/no)" when the run could go on. The run resumes (same
    session, so the chat is its context) only on a plain order (`isGoOnOrder`: continue, resume, go on, retry the
    step...) or a plain yes (`isYes`) right after that question (`askedToResume`); a message with images is always
    chat. A step finished in the chat counts as done and the run starts at the next one. The resumed step's request
    now starts with the owner's words. A password-like message is kept as a plan private detail and not sent to the
    chat. Driven live 2026-11-14 (PLAN-G, Haiku 5.5, about $0.03): 1a blocked (no GREETING_TEXT), run stopped; a
    question in 1a's chat showed as its own bubble, got a short answer, the run stayed stopped; giving the value
    made the agent finish 1a there, mark it [x] and ask "Should I resume the plan run from step 1a? (yes/no)"; "yes"
    resumed in the same session, skipped the done 1a, ran 1b, "2 of 2 done".
  - Owner (2026-11-14): "Finish without them" ended a run as done although steps were still held, so the end-of-run
    review was offered and the held step's chat became a plain chat. Now the button reads "End the run here"; a run
    that ends with held or set-aside steps ends as needs-you ("Run ended: it needs you", plan/runner `endStatus`),
    with "Held steps stay open: answer them in their chat or with Run. The end-of-run review comes once they are
    done."; the review is offered only once nothing is held. Driven live (PLAN-H, Haiku, under $0.02): 1a blocked,
    End the run here gave needs-you with no review offer; in 1a's chat the value finished 1a and the agent asked to
    resume; "yes" ran 1b, "1 of 1 done", review offered; the run kept its first decision (the earlier fix).
  - Owner (2026-11-14): scrolling up to read while the agent works was pulled back down by every new message. Now
    the chat follows new activity only while at the bottom; scrolled up, the place is kept and a "↓ New activity"
    pill at the bottom edge goes back down (another chat or a sent message goes to the bottom too). AgentPane
    `follow`/`newBelow`. Driven live: a reply arrived while scrolled up, the place held, the pill showed and
    took the view to the end.
  - Owner (2026-11-14): PLAN-E 53z asked for the MySQL login again although it was kept for the plan. Cause: the
    step's chat was handed over twice (heavy chat); a chat handover's new session got only the note, never the
    plan's private details. Now a chat in one of the last run's step sessions (runner `privateFor`) gets them as
    the hidden block steps get: with a handover's first message (agent `privateNote`, at the end so not part of
    the shown note) and with messages in a stopped step's chat. Owner, on plain-text logins: plan notes do not
    ship, but secure it if cheap. Done: the stored copy of every request masks the private block's "- " lines
    (agent.rs `mask_private` in `user_line`; details kept to one line each); migration 7 masks copies already in
    the database (driven on the test build with a fake value: masked); a login typed during a running step is kept
    for the plan and the chat stores a placeholder, the agent gets it in the private block. Not covered: Claude
    Code's own transcript files keep what was sent (outside Qanuvi). Published in: not yet (0.1.19 is before it).
- [ ] **24z. The assistant core: headless brain, window optional (owner, 2026-11-14; DESIGN RULES, not a build step
  yet)**: Qanuvi grows into an assistant ("hey Qanuvi, read the finished summary and configure that", "make a new
  plan from these clauses for this plugin or folder", queued tasks and several plans in a row). As it grows, most
  memory goes to the brain (speech to text, voice, local models, indexes), not the window; the window (WebView2 on
  Windows, WebKit on macOS and Linux, about 300 to 470 MB) stays a roughly fixed cost. Owner: focus on the main app;
  a native UI is a much later subject, only if ever needed.
  - Rule 1: the assistant builds on the brain Qanuvi already has, not a new one. Already in the Rust core: brain.rs
    (model per turn, local first for light jobs, skips limited or broken models, falls back), llm.rs and
    local_agent.rs (Qanuvi's own agent and its tools), machine.rs (models that fit the PC), memory and the memory
    disk, the code index. Still in the window (TypeScript): the decider (core/decider.ts, seats and decision log),
    learned routing, the planner, the plan runner, guides and free-model ranking; these move into the core with
    step A. New abilities (voice: wake word, listening, speaking; understanding spoken or typed requests) are built
    in the core and take seats in the same decider (24b), never inside the window, so they work with it closed.
  - Rule 2: the window is one handler among others (like the phone and the laptop through the relay). Everything the
    assistant does must also work headless.
  - Gap today: the plan runner (stores/runner.svelte.ts: step order, holds, set-aside, reviews, closing steps,
    handovers) lives in the window; closing the window ends a run. The headless master (Milestone 16) runs agents
    and terminals without a window, not plans. One run at a time; no queue of plans or tasks.
  - Later step A, headless runner and queue: move the runner's decisions into the core (Rust, or the pure plan/*.ts
    functions run by the core) so runs go on with the window closed and on a master over SSH; a queue of tasks and
    whole plans that run one after another (or side by side where they touch different projects), each with its
    holds and answers kept per plan, with the paid agent on top as monitor (24e); the window, the phone and
    notifications show and answer the same queue (24u part 4 Inbox is its answer channel).
  - Later step B, tray mode: closing the window keeps Qanuvi in the system tray (runs, queue, voice, notifications
    go on) and unloads the web engine completely (all of its memory back while gaming); opening it shows everything
    again. A tiny native overlay ("listening...", a question) only if voice needs one, not a UI rewrite.
  - Later step C, cheap window trims when convenient: draw only the visible messages of long chats (the biggest
    cut), drop cached memory while minimized, load heavy views (Git, diffs, Preview) only when opened. Measure the
    web engine's parts first.
  - Much later, only if ever needed: a native UI (GPUI or Slint), starting with a measured prototype of the chat.
  - Step A plan (owner agreed 2026-11-14): RUST CORE, not an embedded JavaScript engine. Map of today: core/decider.ts
    (343 lines) and plan/*.ts (about 3,900 lines with tests) are pure; stores/runner.svelte.ts (2,454 lines) is the
    window part (api.files 49 calls, api.agent 22, runs/index/checks/git/usage; the agent store's sessions, events and
    mid-step model/mode/interrupt hooks; settings, inbox, projects, planner.ask, memoryContext; localStorage for picks;
    setTimeout for limit waits). master.rs already serves files/git/checks/index/agent sessions headless; runs.rs only
    stores records.
    - [x] R1: port the pure logic to Rust with its tests (decider, tasklist, preflight, routing, packet, setaside,
      holdask, limits, costs, picks, planstate, housekeeping, finishing, guidance, planner, prompts); a comparison
      check runs the same inputs through TypeScript and Rust and must match before TypeScript is removed.
      - R1 part 1 DONE 2026-11-14: module src-tauri/src/plan/ (in the library, so qanuvi-master gets it; builds
        without the desktop feature): decider, limits, holdask, prompts, picks (pickFor; storing moves to the DB in
        R2), costs, setaside, a first routing.rs (ModelProfile only), and js.rs (helpers that behave like the
        TypeScript: String(), toFixed ties, Math.round, slice on UTF-16 units, jsonIn). Each test file ported with its
        code (29 Rust tests). Comparison check: src/lib/plan/compare.test.ts runs 88 cases through the TypeScript
        and keeps src-tauri/src/plan/compare.json current (a file snapshot: npx vitest run -u
        src/lib/plan/compare.test.ts writes it again); plan::compare runs the same inputs through Rust and compares
        the JSON (numbers by value). All 88 match; a changed output was checked to fail. Limit reset times use jiff
        (already in the lock file through another crate; its jiff-static part was fetched once) for time zones.
        Verify: 376 TypeScript + 201 Rust tests (Rust tests -j 1; the parallel run left a corrupt artifact).
      - R1 part 2a DONE 2026-11-14: tasklist.rs (parse and edits; edits return Err with the TypeScript's message;
        findTaskLists sorts like localeCompare for paths: punctuation in the default collation's order, then digits,
        then letters, case only to break a tie), routing.rs (all of it; AgentModel lives here; monthStart takes a
        jiff::Zoned), runner.rs (PlanRun, StepRun, Attempt, RunOptions and every decision; tests in runner/tests.rs),
        finishing.rs (FinishPass and the pass's prompts and readers); prompts' parsePlan test ported; js::trim and
        js::trim_start (JavaScript's white space: the byte order mark yes, U+0085 no). Comparison check: 209 cases,
        all match (a changed output checked to fail). Stats is a HashMap: paidPerStep sums per type in another order
        than the TypeScript Map, so cases use sums exact in binary. Verify: 376 TypeScript + 53 Rust plan tests,
        clippy and svelte-check clean.
      - R1 part 2b DONE 2026-11-14: preflight.rs, packet.rs, planstate.rs, housekeeping.rs, planner.rs, guidance.rs
        (handover.test.ts ported into guidance's tests); js.rs gained trim_end, from_unit (slice from a UTF-16 unit),
        parse_int, cmp_utf16 (JavaScript's default sort) and S / DOT (JavaScript's `\s` and `.` as Rust classes).
        JavaScript regex traits done by hand: the path lookahead in mentions (the end is matched and the next search
        starts where the path ends), the password lookbehind/lookaheads in looksSecret, the lookbehind split in
        sectionTitleFrom and the lookahead split in mergeApi; `\b`, `\w`, `\d` and `/i` kept ASCII as in JavaScript.
        Left for R2 (localStorage today, the DB then): loadPlanState/savePlanState (PlanState::from_json and
        is_empty are ported; from_json drops entries with missing fields, the TypeScript keeps any with a step),
        keepRunSetting/runSettingsKey, loadPlanNotes/savePlanNotes. Known small difference: backticked names in
        mentions are 2-80 characters in Rust, 2-80 UTF-16 units in TypeScript (differs only outside the BMP).
        Comparison check: 429 cases (220 new), all match; three changed outputs checked to fail. Verify: 376
        TypeScript + 286 Rust tests (114 plan tests), clippy and svelte-check clean. R1 is complete.
    - [x] R2 PRE-FIX DONE 2026-11-14 (owner: PLAN-F stalled at its first step in the installed app): git is the
      user's choice, never a must. Causes found in the PLAN-F record: the review took its diff from the outer repo,
      which ignores /content/plugins/* (qCommerce is its own repo inside), saw nothing and rejected real work twice
      ($5.05); the replan numbered 97a's replacements 97a-97d next to the real 97b-97d; the replaced step was held in
      the Inbox itself and its own replacements waited for it; a replaced step never counted as done. Built:
      - snapshots.rs (core, also in qanuvi-master): Qanuvi's own snapshot of the project's files (size, time,
        fingerprint; texts stored once by fingerprint in the data folder, two weeks kept) and unified diffs from it
        (own Myers line diff, no new crate). Follows the file tree's files (nested repos found by themselves) plus the
        plan's folder whatever ignore rules say. Measured on amirularham (about 2,000 files): first 23 s in a debug
        build, 28 MB; every later one 0.3 s. git_blind: the plan folders no repository sees.
      - Project settings "Plan step changes from": Qanuvi's snapshots (default) or git (each file's own repository
        against its last commit); with git, files git cannot see still come from the snapshots, and the run asks
        once in the Inbox (Keep git / Use Qanuvi's snapshots), the run goes on meanwhile. Checked before the first
        step (git_blind), and in each review (unseen).
      - Reviews: reviewWithoutDiff: no diff of files the step changed, or changes that cannot be read, never reject a
        step alone; the tests decide and the reason is said. The chat's free-model review uses the snapshots too.
      - Replans: freeLabels + renumberWith (97a → 97a-1, 97a-2, …; every label a planner wrote before a title is
        dropped); the replaced step is not held and nothing waits for it; once all its replacements are done it is
        ticked (replaces / replacementsDone). The run's start snapshot is kept in the run record (finishing pass
        after a restart).
      - Found by the new scenarios: reopenedSteps took a step the run skipped for an unmet Needs line (still open,
        with its Skipped note) as sent back by the owner, so it was skipped twice and listed twice in the summary.
      - All rules in TypeScript and Rust; comparison check gained the cases, all match.
      - THE STANDING TEST SET (owner: "a running feature must not break while a new one is made"):
        src/lib/stores/runner.scenarios.test.ts drives the real runner store, the real plan logic and changes.ts
        against a fake core (src/lib/stores/testing/fakeCore.ts: files, snapshots, scripted agent sessions, checks,
        run records; an unknown command fails loudly). 10 scenarios: plain run; PLAN-F empty-diff review; review of
        a change in an ignored plugin folder; PLAN-F replan numbering and the replaced step; git-blind question;
        blocked step through the Inbox and answer; failing tests and the fix round; pause after each step; stop
        mid-step; Needs on another plan. snapshots/tests.rs covers the four layouts (no git, ignored folder, nested
        repo, plain repo). Each fix was checked by putting the bug back: every one fails a scenario.
        RULE: every runner bug found in a real run gets a scenario before its fix; a runner change is done only
        when npm run verify (with the scenarios) passes. R2 ports these scenarios to the Rust runner's tests first,
        so the Rust runner must pass the same runs.
      Driven live 2026-11-14 (test build, scratch site like amirularham: the outer repo ignores /content/plugins/*,
      the plugin has its own repo and the plan): the "Plan step changes from" setting saves and reloads (git chosen);
      the pre-run check set the schema step to extra checks; the run did 2 of 2 ($0.06); the review's prompt held the
      nested repo's real diff of src/Schema.php and accepted the step; the run record kept its start snapshot and
      the light finishing pass used it. Finding (older, not from this change): the finishing pass offer said about
      $0.0038 and cost $0.15 (it ran on Opus 5.5); the estimate needs the pass's real model. Published as 0.1.21.
    - [ ] R2: the runner in Rust (step order, attempts, test-and-fix rounds, reviews, replans, holds through the Inbox,
      closing steps, handovers, limit waits; a Rust agent turn that waits for idle/stopped/error); the window gets run
      updates as events and sends pause/stop/model-switch; picks move from localStorage to the DB; the same runner in
      qanuvi-master; the TypeScript runner deleted. Split agreed 2026-11-14 (owner: "i agree on them"): R2a
      foundations (migration 9 for picks, plan state, plan notes and run settings with a one-time import from
      localStorage; Rust memory slice and planner call; a RunEnv trait with stand-ins; the scenarios above ported
      first), R2b the agent turn and its fix rounds, R2c the run loop, R2d wiring (commands, a qanuvi-run event, a
      thin RunnerStore with the same exports, the master's commands and a run stream), R2e drive live on a scratch
      project and delete the TypeScript runner. Decided: run settings go with the start and on change (not all
      settings into Rust now); master runs without a credential store say so in the step and go on (a master store
      later); the TypeScript plan files and the comparison check stay until nothing imports them. Note: migration 8 is
      the Inbox, so the queue (R3) is migration 10.
      - [x] R2a DONE 2026-11-14: migration 9 `plan_data` (project, file, kind = picks/state/notes/run, JSON value; no
        reference to projects, so a master's projects keep their window copy; removed with its project); plan_data.rs
        (typed load/save for the runner, notes kept to 60, run settings patch, one-time import where the database
        wins); commands plan_data_list/set/import and the "qanuvi-plan-data" event. Window: plan/planData.svelte.ts,
        a copy that reads at once and writes through (same keys as localStorage had, so planstate.ts, guidance.ts,
        picks, RunPanel and PlanView only swapped localStorage for it; an older echo never undoes a newer write;
        started first in +page). plan/env.rs: the RunEnv trait (files, snapshots, agent turn that waits for the turn's
        end, checks, usage, run records, Inbox, plan data, memory slice, project settings), CoreEnv over the real
        core, memory_context and ask_planner (read-only turn, memory slice, stopped after, errors as the window).
        runner/fake.rs: the stand-ins (port of fakeCore.ts; Inbox and plan data are the real modules on a database
        in memory). runner/scenarios.rs: the 10 scenarios ported, #[ignore] until R2c connects the `Run` hook.
        Verify: 400 TS + 312 Rust (11 ignored), clippy and svelte-check clean. Runs were done under a memory guard
        (scratchpad guard.ps1): the first full verify hit low virtual memory and blacked out the installed Qanuvi.
      - [x] R2b DONE 2026-11-14: plan/turn.rs (port of the window's #turn and #attempt). `Controls` holds the owner's
        asks to the running step (stop gently then at once, pause, another model, "read my message now", mode),
        set by the commands later (R2d); `StepWatch` holds what happened to the step (limits, handover, over limit,
        pause now, the switched model) for the run loop. `turn` sends a turn and steers it: limits every 15 s,
        handover on a big context, pause/switch/stop messages with their late stops (3 min, 1 min for Stop), the
        30 s hold after an interrupt; a stop asked before a turn starts no agent. `attempt` = turn plus up to two
        fix rounds in the same session (live model and mode kept), Attempt with the step's own share of cost,
        tokens and time. RunEnv's agent turn is now steerable: its callback hears every event and a quiet tick
        about once a second and answers a `Flow` (Go, Hold, Send, Stop, End); CoreEnv sends mid-turn messages on
        the same channel and keeps "process gone" (only the session's sinks hold the sender); TurnEnd carries
        tokens and time. Fake: scripted lines and quiet seconds, messages sent into turns kept. Fixed on the way
        (window runner bug, not ported): a pause during a step with tests ran the tests and sent a fix round; now a
        paused step stops where it was paused. 22 new tests (21 turn, 1 process tree); 334 Rust tests pass (11
        ignored until R2c), clippy clean. TypeScript unchanged.
      - [x] R2b also (owner agreed 2026-11-14), DONE: stopping an agent session stops its whole process tree
        (Agents::stop uses checks::kill_tree; on Unix the agent starts in its own process group). Test: a parent
        with a child, both gone after the stop. Was: Today
        Agents::stop does child.kill(), which on Windows ends only the agent process; a non-lean session's MCP
        servers (node: Playwright, web research) stay running after idle close (10 min), Stop and app exit. Use
        checks::kill_group (taskkill /T on Windows, the process group elsewhere) in stop, stop_project and stop_all,
        with a test that starts a process with a child and checks both are gone. Seen live: 21 old agent chats in
        the owner's VS Code held about 5.5 GB (each with 4 MCP node processes).
      - [x] R2c part 1 DONE 2026-11-15: plan/run.rs, the run loop (port of the window's `start` and its helpers):
        steps in order with their Needs lines (other plans and the archive too), holds before a step, the model
        choice (one model, Pick models, Model lines read fresh, rules, learned stats, budget), free first with the
        decider's assignment, attempts through turn::attempt with the model ladder, handovers, the owner's model
        switch and mode, pause in a step and after it, the step's limits, usage-limit waits (or free models until the
        reset), reviews from Qanuvi's snapshots, the planner's rewrite, held/blocked/failed steps set aside with the
        Inbox question and the plan's and decider's set-aside calls, closing steps run again, steps added mid-run,
        notes, API.md, the step logs, private details, the end message and Inbox item. `Shared` is the running
        loop's handle (Stop, Pause, answers, End the run here; Controls for model, mode, "read my message now"; run
        and agent-event listeners for R2d). Answers come from the window or straight from the Inbox (looked at after
        each step and while waiting), and a taken answer settles its question, so a step held again is asked anew.
        `on_answer` applies the changes-from answer (R2d calls it when an item is answered). RunEnv gained
        session_lines, plan_private(_clear), outline, find_symbols, free_decide and list_all_runs; CoreEnv gained
        the code index. The 10 scenarios run on it unchanged except the changes-from one (calls on_answer). New
        scenarios: pause mid-step keeps the session, usage-limit wait then retry, the owner's model switch hands
        over, private details reach steps and go when the plan is done, an Inbox answer given while no run went on
        counts at the next start. 350 Rust tests pass (1 old snapshot test ignored as before), clippy clean.
        Fixed on the way (window runner bugs, not ported): a pause with carry-on off stopped the session kept for
        Resume (closed before the pause was handled); the owner's model switch was taken for a context handover
        (switchingTo was cleared by #attempt before the runner read it), so it counted against the handover limit
        and got the wrong resume note; an answer given in the Inbox while no run went on was lost. Not in part 1:
        the finishing pass (R2d, with its commands), owner messages (part 2).
      - [ ] Replaced steps get their own colour (owner agreed 2026-11-15): a step marked [!] only because the planner
        rewrote it (its "Replaced by steps ..." note, planner.ts replaceStep) shows in the Plan view with its own
        colour (not the red of a real block) and the label "replaced by 97a-1–97a-4"; it turns green when all its
        replacement steps are done (closeReplaced already ticks it [x]). Seen live: PLAN-F 97a showed red while its
        replacements ran fine. Small window change; next build.
      - [x] R2c part 2 DONE 2026-11-15 (core side; the window sends to it in R2d): owner messages in step chats.
        plan/ownermsg.rs: the sorting (bare stop/pause words at once with no model; else the free decider, Qanuvi's
        own models; else word rules), the texts to the agent and the chat, the "now / after this step / new step" Inbox
        form, reading an answer in words, and add_step (the change as its own step right after the step at work, a
        free number, "Needs:" that step). turn.rs: Controls::owner_message queues a message; the turn takes one per
        tick: a note or question is sent to the agent as such and the step goes on; stop/pause act as the buttons; a
        change holds the agent after the edit in hand and asks in the chat (a "qanuvi_runner" line, shown as a notice
        in events.ts) and the Inbox, waits (run option messageWaitMinutes, default 10) with the step's limits not
        counted, then takes the answer (chat words or Inbox) or the safe choice (a new step); a second change while
        one is asked becomes a new step at once; a turn that ends while asked also gives a new step (nothing lost).
        run.rs: Shared::owner_message; each message into the step's log and the run's decisions (kind "owner
        message", who sorted it, how it was handled); new steps written into the plan and taken next. Fake: a
        scripted free decider, a tick hook, and a message sent to a turn waiting at its end runs the agent again in
        the same session. 6 new scenarios, 2 turn tests, 7 ownermsg tests; 365 Rust tests pass (1 old ignored),
        clippy and fmt clean; events test 35/35. For R2d: the window's chat on a running step sends to the runner
        (not api.agent.send), shows the owner's own message, and keeps the runner's notices in the session's lines
        (today they reach only the live listeners).
      - R2d split (2026-11-15, the size of R2c): part 1 the core side (run handles, commands, events, the finishing
        pass, the master's commands and stream); part 2 the window (a thin RunnerStore with the same exports over the
        commands, the step chat sending to the runner, the runner's notices kept in the session's lines, standing
        owner rules); part 3 what part 2 leaves (the master's Inbox, see below). R2e drives it live.
      - [x] R2d part 1 DONE 2026-11-15 (core side; the window still runs its TypeScript runner until part 2):
        plan/live.rs: `Runs`, one `Shared` per project, a run or a finishing pass on its own thread, one at a time
        per project ("A plan run is going on in this project."); `RunView` (the run, running/finishing, waits, the
        model at work, notices, pausing, stopping, checking, the step's session, messages not taken yet, the last
        error); `Tell` (where the view and the steps' agent events go, each event with its session, reviews and
        planner turns too); `RunStart`, the start as the window sends it (camelCase, Settings > Team as stored,
        rules by job name, empty or unknown ones left out); `last_run` from the records. The finishing pass ported
        (`finish_pass`: full checks, the paid agent's review of the run's diff from its start snapshot, free models
        when it cannot answer, the full level's own session with the run's step limits, its Still open list,
        FINISH.md, the Inbox item; Stop at any point ends it as stopped). Shared tells listeners when its waits,
        model or notices change (`touch`), not only when the run is saved. Desktop (desktop_runs.rs): commands
        plan_run_start / finish / state / active / stop / pause / finish_without / answer / message / interrupt /
        model / mode; events "qanuvi-run" (the view) and "qanuvi-run-agent" ({projectId, sessionId, event}); an
        Inbox answer calls run::on_answer for its project; the core's new Inbox questions show a desktop
        notification (only new ones: CoreEnv's notice now says `new`). Master (master_runs.rs): the same plan_run_*
        commands plus plan_run_watch; a run goes on when the desktop disconnects; watching connections get
        {"kind": "view" | "agent" | "event", ...} on their stream until they go; keys are the master's own
        (QANUVI_KEY_..., as agent_start). Fixed on the way: a turn's first event reached listeners before its session
        id was known. Tests: plan::live (thread, one at a time, views and agent events; light pass; full pass; Stop
        during the checks; the window's start shape) and a master run streamed to a watching connection.
        Not ported, on purpose: the window's per-file git diff for a run with no snapshot (very old runs; the review
        goes by the files and Done notes). Left for part 3: the master has no Inbox commands, so a master run's held
        steps wait in the master's own Inbox, which the desktop does not show yet.
        371 Rust tests pass (1 old ignored as before), clippy clean on the desktop and the master-only build, fmt
        clean. TypeScript unchanged, nothing published, no live check (the window calls none of it until part 2).
      - [x] R2d part 2 DONE 2026-11-15 (window side): stores/runner.svelte.ts rewritten as a thin store over the core
        (about 640 lines instead of 2,600; same exports for RunPanel, PlanView, AgentPane, PlannerDialog). start and
        finishPass send a RunStart built from this window's settings (agent, models and labels, Team, rules, monthly
        budget, memory, effort, Qanuvi's own model setup; plan_run_start/finish fill in the keys from the credential
        store, as the master does); Stop, Pause, answers, End the run here, model and mode pickers, "read my message
        now" go to their commands; the view ("qanuvi-run") sets the panel's state, puts new notices in the chat once,
        and the chat follows the new step session (live.rs now tells the view when a step session starts). Agent
        events ("qanuvi-run-agent") go to the chat; a session not open there (reviews, planner) is only listed, so its
        end no longer ends the open chat's turn. Resume of a paused step and "continue" in a stopped step's chat go
        as the start's resume (paused: Qanuvi's note; continue: the owner's words). Pick models, learned stats and
        private details stay in the window. Step chat: the owner's words show at once (agent.showOwn) and go to
        plan_run_message; the runner's copy to the agent shows as the same message, once (events.ts unwraps the
        ownermsg wordings; test with Rust's exact texts); files cannot go to a running step (said in the chat); a
        login typed there goes to the plan's private details and plan_run_tell (new: Controls::tell_agent, sent as
        it is, no model sorts it). Runner lines ("Taken as a note ...") are kept in the step session's stored events
        (RunEnv::keep_line), so a chat opened later still shows them. The old TypeScript runner's scenario test and
        fake core are gone (R2 ported the scenarios to Rust). Not driven live (R2e).
      - [x] R2d part 3 DONE 2026-11-15 (the master's Inbox, and the window following a master's run): master_inbox.rs
        gives the master the desktop's inbox_* commands with the same arguments (list, answer by id or key, close,
        resolve, mark read with no id = all but open questions, remove, clear) plus inbox_watch, a stream of
        {change, item} (the "qanuvi-inbox" payload) until the connection goes. An Inbox change a master run makes
        (CoreEnv's notify) goes to these watchers too, so a desktop that does not watch the run still sees its
        questions. An answer on the master applies run::on_answer there (changes-from), as the desktop does; a held
        step needs nothing more (its run reads the master's Inbox). Window: stores/inbox.svelte.ts merges this
        computer's items with each connected master's (newest first, each marked `master`); answer, remove and mark
        read go to the machine that has the item, "all read" and Clear all to every machine; the master's answers
        reach onAnswer listeners; an older master without the commands is left as before. masters.svelte.ts follows
        a master's Inbox on connect and drops its items when the connection ends. A new question on a master shows
        a desktop notification once (new command notify_item; Notifier::first_time keeps the last 200 ids, as every
        window following the master asks). Found on the way, fixed here: part 2 never followed a master's run (the
        window only heard this computer's "qanuvi-run"), so a run started on a master showed nothing. runner.svelte.ts
        now calls the master's plan_run_watch once per project per connection (start, finishing pass, last run) and
        takes its view and agent events from that stream; on connect, a run going on there is shown when nothing runs
        here; when the connection ends the panel stops showing it as running and says the run goes on there.
        Not done, noted: plan data (holds, answers kept per plan) is not routed to masters (planData.svelte.ts calls
        this computer's core), so the Plan view of a master project does not show the master's held-step list; the
        Inbox does. A hold answered in the Inbox of a master project is not kept in this computer's plan state (it
        would be the wrong machine's). Tests: master test "its_inbox_is_read_and_answered_from_the_desktop"; vitest
        stores/inbox.test.ts (merge, routing, notification once, older master). Not driven live (R2e).
      - [x] R2e part 1 DONE 2026-11-15 (driven live on this computer; master paths are part 2): a scratch Node project
        (PLAN-R2E.md, 19 steps in 5 sections), test build, 4 runs. Worked: pre-run check (a risky step set to "Wait for
        me" held it; 2b set aside behind 2a by the same-step-number rule), the held step answered in the Inbox page
        and the run went on, a question in a step chat ("Taken as a question", answered, step went on), a change
        ("as a new step" typed in words made step 5b-1 with the owner's words), Pause (agent ends the edit, writes a
        progress note) and Resume in the same session, the model switch (Haiku stops after the edit, Sonnet goes on
        with the handover note), the light finishing pass with checks and a still-open list, a desktop notification
        for the change question, and the run going on while the window was closed until the process ended.
        Bugs fixed: (1) the run was published only after a step's attempt, so the window never saw the step at work:
        a model picked for it and words typed into its chat never reached the runner (the chat queued them as plain
        follow-ups), and the panel said "try 2" on a first try; run.rs now saves before each attempt; scenario
        "the_window_sees_the_step_at_work_as_running_while_its_agent_works". (2) Closing the last window ends the
        process (by design until R4), but its run record stayed "running": the reopened window showed a step at
        work that nothing ran and offered Run, not Resume; live::last_run now reads such a record as stopped, its
        step as waiting and a finishing pass as stopped, as the TypeScript runner did; test in live_tests.rs; driven
        live (Resume shown, run resumed). (3) Plan view spinners on steps already done: a session that goes on
        across steps keeps the first step's title; the title fallback now applies only when no run of the plan goes.
        Open, small, for the owner to choose: (a) the light finishing pass was offered "about $0.0024" and cost
        $0.19 (the estimate uses the steps' median cost, cheap Haiku steps, but the pass runs on Opus; proposal: scale
        by the pass model's price); (b) preflight reasons are cut at 300 characters mid-word with no "…" (js::cut);
        (c) "Pausing (finishing its edit)…" squeezes the plan header so the file name and Open file wrap; (d) a
        change made into a step keeps the "Change:" prefix and an ellipsis in its title; (e) "(try 2)" still shows
        during a step's after-work (notes, checks) once its first attempt is recorded.
      - [x] R2e part 2 DONE 2026-11-15 (master paths, driven live with a "This PC" master): Servers, SSH address
        "local", a scratch folder added as a server project (PLAN-M.md, 3 steps; 1b deletes README.md). Worked: the run
        started on the master (its agent a child of the master service), the window followed it (step chat, Pause and
        Stop, spinners), 1b "Wait for me" held it, the question showed in this window's Inbox (and a desktop
        notification), "Go ahead" there reached the master and the run went on; Disconnect while 1c worked: the
        project left the sidebar, the master finished the run alone; Connect again: the finished run, its cost and the
        finishing pass offer shown. Fixed (test first): (1) "This master does not know the command files_markdown":
        the master now answers files_markdown, files_changed_since, files_log_append, files_replace, files_search with
        the window's options (whole word, include, exclude), plan_data_list and plan_data_set (its own database, where
        its runner reads them) and plan_private, plan_private_add and plan_private_clear (its own credential store; a
        server build has none and says so); master test "answers_the_plan_views_file_and_plan_data_commands".
        (2) planData.svelte.ts called this computer's core for every project, so holds, picks and run settings set in
        the Plan view of a master project never reached the master's runner: it now sends them to the master that owns
        the project, takes that master's plan data on connect (masters.svelte.ts) and its runner's changes from the run
        stream (runner.svelte.ts, "qanuvi-plan-data"); vitest in planData.test.ts. (3) Sidebar: many projects ran
        under the Settings row (the projects group did not scroll); it scrolls now (rebuilt and seen in the test build).
        Findings for the owner (not built, proposals):
        (f) The master chose Sonnet for steps the Plan view showed as "Auto · Haiku 5.5": the label learns from this
        computer's runs, the runner from the runs of the machine it runs on (a new master has none, so the default
        model). Proposal: the run's start carries the window's learned model stats (and month's spend) to the master,
        so a run chooses the same models on any machine and the label stays true; the master adds its own runs to them.
        (g) The pre-run check decided "run 1b after 1a and 1c" (README must list the final exports), but the runner
        kept file order: 1b ran before 1c, its hold also held 1c ("1 step waits for it"), and 1c's agent had to add its
        line to the README itself. Not master-only. Proposal: an order decision of the check is written as a
        "- Needs: 1c" line under 1b (the plan already reads Needs lines), shown in the check's list so the owner sees it.
        OWNER DECISIONS 2026-11-15 (on (f)-(i) and masters):
        - Every Qanuvi needs the account, on every platform (PC, master, mobile): any PC can be a master and a phone
          can hook into it, so they are the same. The account is not built yet: a placeholder that is bypassed for now.
        - The account keeps the owner's behaviour and the data that syncs to every device, also separate ones.
        - Workspaces never sync, in either mode: workspaces, plans, schedules, notes and automations belong to the
          installed copy (the machine). Synced mode only adds shared information from both sides on top.
        - Two modes per master, chosen at setup, changeable later: SEPARATE (default) and SYNCED. Separate: this
          computer is only a window on the master; the master keeps its own history, memory, plan data and settings,
          and everything the window shows about it (model labels, costs, history) comes from the master. Synced: the
          same account keeps learned model history, plan data, memory and rules the same on both; passwords and keys
          never sync. Either machine can turn syncing off for itself.
        - In both modes the owner's local brain (what Qanuvi knows of the owner) is shared through the account. How a
          separate master knows the owner is still open: design it with the milestone.
        - (f) is covered by the modes: separate, the label reads the master's own history; synced, both have the same.
        - (g) agreed: an order decision of the pre-run check becomes a "- Needs:" line, shown in the check's list.
        - (a)-(e) from part 1: fix all as proposed.
        - (h) the window title after a run ends: fix. The "heavy" bar at 26k was not a bug: the Chat handover setting
          (ask / auto / off and a size, default 150k) was set low in the test build.
        - (i) a step's chosen model goes into the plan's own data (plan memory), not into the plan file.
        - New milestone (to write out in parts, owner reviews before building): master accounts and the two modes.
        (h) Small: after a run ends the window title still names its first step ("Step 1a: ..."); the "This chat is
        heavy" banner shows at 26k context. (i) Note: a step's model picked in the Plan view is written into the plan
        file as "- Model: <id>" (set_step_model); in part 1 an agent read those lines as model attribution in the
        project. Keep it in the file, or move it into plan data?
      - [x] R2e FIXES DONE 2026-11-15 (owner decisions above; tests only, not driven live yet): (a) the finishing
        pass estimate scales each step's cost by the pass model's price over the step model's (output price per
        million; finishEstimate/finish_estimate take PassPrices; RunPanel passes the planner model's price). (b) check
        reasons over 300 characters end at a word (from 200 on) with "…" (clipReason / clip_reason). (c) the Pause
        button says "Pausing…" (full words in its tooltip) and never shrinks or wraps. (d) a new step's title from the
        owner's words loses "Change:" / "New step:" labels, a closing "as a new step" and any ellipsis, and a long one
        ends at a word (ownermsg::tidy_title, used by add_step). (e) once an attempt finished the step, its after-work
        shows that try, not "(try 2)". (g) the pre-run check answers an "order" list ({step, after, why}); the check's
        list shows it and, on start, addOrderNeeds writes each as a "- Needs:" line (only steps in the plan, never a
        circle). (h) a session that goes on into the next step is renamed to that step (the window title follows it);
        fix rounds keep the title. (i) the model chosen for a step in the Plan view, by a switch during a run, or by
        "Keep it for every open step" in the Run panel goes into the plan state ("models": step, title, model), not
        the file; a "- Model:" line the step had is taken out then; a hand-written "- Model:" line still wins over a
        kept choice; kept models stay while the step is in the plan. The planner too (owner's yes, 2026-11-15): a
        new plan (planner.svelte.ts accept) and a replanned step (run.rs) go through takeModels / take_models after
        numbering: "- Model:" lines leave the block and each goes into the plan state under its final number; plans
        written before keep their lines (hand-written and planner lines cannot be told apart; picking a model in the
        Plan view moves one). Commits 6fa3523, fd705f6 and the planner commit after them. NEXT: drive (a)-(i) live in a test build on a scratch project, then delete what is left of
        the TypeScript runner; then the master accounts milestone written out in parts for the owner's review.
      - [x] R2e fixes driven live 2026-11-15 (test build, scratch PLAN-R2F, about $0.25): (i) a Plan view pick goes
        to plan state and takes the step's Model line out of the file; a hand-written line wins over a kept pick.
        (g) the check's Order list ("6b after 6c") became one "- Needs: 6c" line, not doubled by a second check.
        (c) "Pausing…" at full width. (d) "Change: … as a new step" typed during 6c, answered "As a new step" in the
        Inbox: step 6c-1 titled without the label, the closing words or the ellipsis. (h) sessions carry the step
        they went on into. (a) finishing pass offered at $0.17 / $0.65 for a $0.11 run. Fixed on the way (scenarios
        first): a step needing a LATER step of the run now waits (stays open) and runs after it; only later steps
        count, so two steps needing each other end skipped (run.rs ready; scenarios
        a_step_that_needs_a_later_step_of_the_run_waits_for_it_and_runs_after_it,
        two_steps_that_need_each_other_are_skipped_and_the_run_ends); driven live: 6b, 7b, 7a, 3 of 3 done. The
        Inbox note of a paused run says "paused". Sidebar: projects and sessions share the free height 2:1, each
        capped at its own content and scrolling inside. Not reached: (b), (e).
      - [x] (agreed design, kept for reference) Owner messages in step chats (owner agreed 2026-11-15; build into R2c/R2d, Rust runner): a message the
        owner types into a chat the runner opened goes to the runner first, not straight into the agent's task. The
        decider (a cheap model) sorts it: (a) a correction or fact for this step: passed to the agent as a note, work
        goes on; (b) a question: the agent answers it briefly in the chat, then goes on; (c) a change of plan or scope:
        the agent finishes the edit in hand, and the runner asks in words (chat + Inbox): now, after this step, or as a
        new step; (d) stop or pause words: as the buttons. No answer within a set wait (setting, default 10 min): the
        safe default (never destructive), e.g. finish the step and add the request as a new step, and it says what it
        chose. The step's log keeps each message and how it was handled. No more pausing a step only because the agent
        went quiet after the owner's message (stopgap in 0.1.22: it is told to go on). Scenario tests first.
      Owner findings to fix with R2 (2026-11-14, installed 0.1.21):
      - [ ] A held step's Inbox question says only "is blocked / it needs you" (PLAN-F 97a, held by the 0.1.20 run
        after two turned-down reviews). The question must say why it was held (who held it, the review's reason or
        the last error, attempts and cost) and what each answer does, in plain words. Build it into the Rust holds.
      - [ ] Right-click > Paste in the Inbox closes the Inbox: Sidebar.svelte closes the popover on any pointerdown
        outside `.inbox-pop`, and the right-click menu (`.cmenu`) is drawn outside it. Treat `.cmenu` as inside,
        and check that the pasted text lands in the answer box.
      - [x] FIXED 2026-11-14 (late): PLAN-F was stuck again on 0.1.21 (every rerun "0/9 done, 9 skipped" in seconds).
        Three causes, all fixed in TypeScript and Rust with tests and comparison cases (unpublished):
        (1) the review of 97a-2 saw an empty diff (the work came from an earlier try) and searched from the root, where
        .gitignore hides the plugin, so complete work was turned down twice; the review prompt now names the plan's
        folder ("search there by path") and, with an empty diff, gives the step's Done line to check against the files;
        (2) renumberWith did not strip labels like "97a-2a." and never rewrote Needs lines, so 97a-5..7 kept "Needs:
        97a-2a" and everything after them was skipped; now each planner label maps to its new number in the block's
        Needs lines; (3) the review's answer was a Headroom placeholder ("why": "<<ccr:...>>"), so nobody saw the
        reason; a result with a placeholder gives way to the agent's last text (planner.svelte.ts and env.rs).
        PLAN-F fixed by hand: 97a-2, 97a-6, 97a-7 marked [x] (97a-2's Done line proves the work), Skipped lines removed.
        Was: PLAN-F rerun on 0.1.21 (22:01-22:17): the replan renamed the planner's 97a1-97a4 to 97a-1-97a-4 but
        left their Needs lines as "Needs: 97a1" etc., so 97a-2-97a-4 were skipped as "needs step 97a1, which is not
        done", and 97b-97z after them. The renumbering must rewrite Needs lines inside the replacement steps too
        (and accept the planner's own label as the same step). Add a scenario first.
    - [x] DONE 2026-11-15 (R2d part 2; ownermsg Kind::Rule, word rules for "always/never/from now on/every step",
      the decider's prompt names it; kept in FACTS.md as "- Owner rule: ..." and in the run's notes, secrets never;
      scenario: a rule said in step 1 reaches step 3's prompt on another model, a one-off note does not).
      R2d also (owner agreed 2026-11-15, part of 24z2): standing owner rules. A note in a step's chat that is a
      rule for the whole plan ("always follow WordPress core style", "check the other plugins before adding helpers")
      is saved into the plan's memory (FACTS.md, "Owner rules"), so every later step and model sees it, not only that
      session; a one-off correction stays with its step. The decider tells them apart (one more kind in
      ownermsg's sort: "rule"). Scenario first: a rule said in step 1 reaches step 3's prompt on another model.
    - [ ] R3: the queue (DB table, migration 10): tasks and whole plans, one after another per project, side by side
      across projects, holds and answers per plan, a hook for the paid monitor (24e, built later). Waits for 24z3
      (owner, 2026-11-15): the queue is owned by its machine, lives in that machine's own database, and its scheduler
      runs in that machine's core; the window adds to and shows the queue of the machine it shows. R3 proposal (not
      agreed yet, discuss again after 24z3): task = one-step plan in .qanuvi/tasks/TASK-<id>.md; done with skips goes
      on, stopped or error holds that project's queue; after a restart the running item comes back stopped (Resume),
      waiting items go on with one Inbox note; run settings saved when added ("Use current settings" on waiting items).
    - [ ] R4: tray mode (step B moved in): closing the window leaves a visible tray icon, runs and the queue go on,
      the web engine is unloaded; the icon opens everything again. Owner: never hidden from the user.
    - [ ] R5: drive live: a scratch plan, close to the tray, reopen, progress intact; then the VPS master over SSH.
    - [ ] R6 (LATER, owner 2026-11-16, after R4): leftover processes. Seen 2026-11-16: old agent sessions, their MCP
      servers (npx/node) and old monitor scripts stayed running and held memory. Today a normal stop or close cleans
      up (agent stop ends its tree, a window takes its terminals/LSP/DAP, the last window ends everything, Headroom and
      OmniRoute stop with their workers), but after a crash or a forced kill only the browser goes (kill-on-close
      job). Proposed: every child Qanuvi starts in a kill-on-close job (Windows) / process group (Linux, macOS), and a
      start-up check for leftovers of an earlier run. OWNER RULE: the tray service (R4) keeps runs, the queue and other
      background work going with the window closed; cleanup must NEVER end what the tray service is still handling
      (its runs, agents, terminals, servers, its own core) — only processes no living part of Qanuvi owns.
    - [ ] R7 (owner asked 2026-11-16, not built; order to agree): "Notes for this plan" on the Run the plan screen. A
      plain text box (several lines) where the owner writes anything every step should know, in their own words: a
      MySQL user and password for the test database, a URL, "never touch the live site", and so on. Plain text is fine:
      it is the easy way when the owner does not want to set env variables in the project's properties.
      Proposed shape: it builds on the plan's existing PRIVATE DETAILS (logins kept in Windows' credential store, given
      to every step as is, never sorted by a model, "Forget", forgotten when the plan is finished if ticked): the box
      is one more private detail, so a password in it is kept the same safe way, NEVER in the plan file, FACTS.md,
      the plan's memory, the database or git. On a master it is kept on that machine (24z3). Every step's prompt gets
      it as one block ("Notes from the owner for this plan: ..."), on every agent and model, the decider too; a change
      during a run applies from the next step. A short line under the box: "Kept with this plan in <machine>'s
      credential store, not in the plan file or git; every step gets it." Left out of logs and Inbox texts. To check
      when building: where a Linux/macOS master keeps private details. Scenario: a note with a test DB login is set,
      step 2 on another model connects with it; the plan file and git show no trace of it.
- [ ] **24z3. Machines: the window shows one machine at a time (owner, 2026-11-15; DESIGN MILESTONE, before R3;
  parts written out for the owner's review, nothing built yet)**: every Qanuvi is a machine ("This PC" too: the desktop
  runs the same core as a master). The work lives on a machine; the window owns no work, it shows one machine and
  sends the owner's clicks to it. Owner chose the switcher over one mixed list (2026-11-15).
  - Owned by each machine (its own database, never mixed with another's): projects (workspaces), plan files and plan
    data, the queue and the runs, the Inbox (questions and answers), schedules, notes and automations, terminals and
    agent sessions running on it; in separate mode also its own model history, costs and settings.
  - Owned by the window (this install): only the switcher (the list of paired masters) and which machine is shown;
    everything else on screen, Settings included, is the shown machine's (Part D). One exception: the Inbox screen
    (Part C2).
  - Owned by the account: the owner's local brain and behaviour; in synced mode the shared history on top. Passwords
    and keys never sync (as decided for the master accounts milestone).
  - Today (to change): the sidebar mixes machines, a master's projects sit in the same list with a server icon
    (Sidebar.svelte, p.master); the Inbox view gathers every machine's questions (kept, with badges: Part C2).
  - Part A, one path to the shown machine: every screen (projects, files, git, terminals, agent, Plan view, Run panel,
    Inbox, queue) calls "the shown machine", never invoke or a master directly; This PC is a direct call, a master goes
    over its connection (relay or SSH). Map first which stores call invoke, which already pick a master, and which
    commands a master does not answer yet (list them; a missing one says so on screen, never falls back to This PC).
  - Part B, the switcher: at the top of the sidebar ("This PC", each paired master by name, its connection state).
    Picking one switches the whole interface: its projects, queue, Inbox, Plan view, runs, history and costs. Tabs,
    editors and terminals are kept per machine and come back on switching back. The machine shown is remembered.
  - Part C, other machines keep working: runs on This PC go on while the VPS is shown (and the other way). The window
    keeps listening to every connected machine's Inbox: the switcher shows a badge per machine ("VPS 2"), a desktop
    notification names its machine, clicking it switches there and opens the question.
  - Part C2, one Inbox screen for all machines (owner, 2026-11-15): the only shared view. Questions stay stored on the
    machine that asked them (nothing copied or synced); the Inbox screen lists every connected machine's questions,
    each with a machine badge, filter "All machines" or one machine. Answering works from the list without switching;
    the answer goes to the owning machine. Only "Open the step" / "Open the plan" switches to that machine. A machine
    not reachable shows its questions as "VPS not connected" until it is back, never an old copy that can be answered.
    The switcher badges and notifications stay. Costs nothing extra: Part C already keeps every Inbox stream.
  - Part D, Settings is the shown machine's too (owner, 2026-11-15: "you switch the PC, the whole screen becomes that
    PC's"): no fixed window group, no split. With the VPS shown, the whole Settings page is the VPS's (models,
    routing, team, budget, keys, startup, backups, data folder, and also its appearance, layout and notifications,
    kept on the VPS). The page header names the machine ("Settings: VPS"). Keys typed there go to that machine and
    are kept there, never sent back (a master gets its own key store; today it reads QANUVI_KEY_... only). Only the
    switcher itself (the list of paired masters and how to reach them) stays with the window.
  - Part E, a machine not reachable: the switcher shows it offline; picking it shows "VPS not connected" and
    Reconnect, never a stale copy taken for live. Its work goes on there.
  - Part F, drive live: This PC and the "This PC" test master side by side on scratch projects only; then the VPS.
  - Agreed with the owner 2026-11-15:
    - Unsaved edits on switching away: kept as drafts on the machine they belong to, back on return; no question asked.
    - A master on an older Qanuvi version: not a subject now (not released, one user); no version gate.
    - Two screens on one machine (two devices, or two windows): both see the same live state, an answer from either
      counts, one question is never asked twice. LATER, to explain to the owner then: a master setting "one screen at
      a time" or "many screens".
    - Connection lost while a machine is shown: an overlay "VPS connection lost, reconnecting..." over a read-only
      screen, gone by itself when it is back; the work goes on on the machine.
    - Several machines at once: "Open in a new window" on each machine in the switcher's right-click menu (builds on
      15c, windows.svelte.ts). Each window has its own switcher and shows one machine; the app keeps one connection
      per machine for all its windows; a machine's streams go only to the window(s) showing it. A badge or
      notification brings forward the window already showing that machine, else switches the current one. On reopen
      each window comes back on its machine. Each extra window is its own web engine (estimate 100-200 MB; Part F
      measures it).
    - Working icon in the switcher: an icon per machine that shows its agents are working (with the count on hover);
      one number per machine, never its chats.
  - Later, not in this milestone: moving work between machines ("send this queued plan to the VPS") as an explicit
    copy, never a sync.
  - This PC always stays in the switcher, never hidden (owner, 2026-11-15): every PC has its own workspace, used or not.
  - Light with many masters (owner, 2026-11-15): a machine not shown is wiped from the window completely (no live
    editors, chats or terminals kept); picking it again rebuilds its screen from the machine (1-2 seconds on a good
    connection is fine). A hidden machine costs only its connection, its Inbox stream and a "run ended/stopped"
    notice; its run and agent streams reach the window only while it is shown. Terminals and agents go on on the
    machine and the window picks up their recent output on return. Part F measures the window's memory with several
    masters connected.
- [ ] **24z2. Tidy, safe code and write scope for plan runs, any project and any language (owner agreed 2026-11-15;
  safe code added the same day; build after R2, on the Rust runner)**: AI agents add helpers that already exist under other names, scatter code
  over files, and edit folders they should not touch. Qanuvi fixes that by itself, with zero noise: no tidiness
  questions to the owner, only a line in the step's log and one in the run summary. Nothing assumes a framework or a
  language (not Qlopy, not WordPress, not PHP): it learns each project. The owner steers through the plan text and
  chat notes ("follow how the core and the other plugins do it"); what is learned once stays in the plan's memory
  for the whole run, on every model.
  - [ ] 1. Project atlas and reuse brief: at plan start one scan with no model: what is shared (found by how code is
    used, not by folder names), the existing functions and classes (code index: PHP, JS/TS/Svelte/Vue, Rust, Python,
    Dart, C#, Go, Java/Kotlin; a language server where installed; plain text for other languages), installed
    libraries from the manifests, the project's naming and grouping, and read-only places the plan or the owner names
    (core, other plugins, vendor). One model call (paid or free) writes a short REUSE.md into the plan's memory (next
    to MAP, FACTS, API): helpers and patterns that fit this plan, where new shared code belongs, how things are named.
    Every step reads it; written again only when the owner points at a new place. The atlas lives in Qanuvi's data,
    not the repo (writing it into the repo is an option).
  - [ ] 2. Write scope: a plan inside a folder (content/plugins/qcommerce/PLAN.md) writes only inside that folder; the
    rest is read only. A plan at the root writes anywhere except protected areas (core and third-party code the atlas
    finds, plus anything the owner marks). Outside edits are the owner's decision: asked once, as early as possible
    (at plan start for steps that clearly need it, in one question; mid-run the step is set aside with one question and
    the run goes on). Answers: allow for this plan (plan memory), always for this project (project memory, given to
    every later plan that touches it), or no (the plan builds on the core from its own folder). Enforced after every
    step from Qanuvi's own snapshots (no git needed): a change outside the scope is undone and kept aside, one
    question: keep or drop. Stopped at write time too where the agent's permission hook allows (check per agent).
    Project permissions listed in the project settings, removable.
  - [x] 3. Standing owner rules from chat notes into plan memory: built in R2d part 2 (see 24z R2).
  - [ ] 4. Step prompt rules (reuse first; new shared code where this project keeps it; what the step may write) and a
    find_similar tool for every agent, the Qanuvi agent included.
  - [ ] 5. Tidy check after each step, no model unless a candidate is found: new functions like existing ones (name
    words, parameters), copied code (fingerprints of normalised text, any language), the same helper twice, a general
    helper in the wrong place, a new function nothing calls. The cheap decider (free first) reads both bodies only for
    a candidate; a confirmed duplicate gets a fix round in the same session ("use X from path, remove Y, keep the tests
    passing"). Old duplicates are left alone (no risky merges without the owner).
  - [ ] 6. Finishing pass sweeps everything the plan added (duplicates across steps, scattered helpers, dead code) and
    fixes them the same way; the run summary gets one line ("Tidy: 2 duplicates replaced with existing helpers").
  - [ ] 7. Safe code, sensitive paths: the atlas also finds the code where harm is done when it breaks or leaks, by
    what it does, not by one framework's names: money (payments, prices, orders, refunds), secrets and keys, tokens
    and sessions, sign-in and permissions, personal data, input from outside (requests, headers, uploads), logs,
    database changes. Found per project in any language from what the code calls (payment SDKs, crypto, session and
    auth APIs, SQL, file writes, header reads) and its names; the owner can add or remove paths in plain words.
    Kept in project memory, given to each plan in its memory.
  - [ ] 8. Safe code, checks on every step's diff: a change that touches a sensitive path always gets the extra
    checks (full tests, the decider's review, never free-model-only work). The review is told what to look for:
    weak or fixed fallbacks for secrets, secrets or personal data reaching logs, trusted client headers (IP,
    Referer, Host), missing escaping or input checks, raw SQL with outside input, a weaker copy of a safe helper the
    project already has. Plus the project's own analyzers when they are installed (found from the manifests and the
    project's config: linters, type checkers, security scanners for its languages); Qanuvi runs them on the changed
    files with no model, and their findings feed the same fix round. Nothing is installed without the owner's yes.
    A confirmed problem gets a fix round; one that needs a decision (a behaviour change for users) is asked once.
  - [ ] 9. Safe code, a release gate: a project can name a staging copy (a folder, a second site, a branch) and the
    paths to try there in plain words ("checkout and payment, sign-in, order emails"). At the end of a run the
    finishing pass copies to staging only if the owner allowed that, runs the tests there, lists what to try by hand,
    and stops at "ready for your approval": Qanuvi never ships to a live site by itself. Backups and a way back
    named before any update. One project's staging is not another's: nothing here assumes a shop, PHP or one host.
  - [ ] 10. Benchmark: sample projects in several languages that already have helpers and some unsafe spots, plans that
    tempt rewriting helpers, editing the core and adding weak fallbacks; checks on and off; count duplicates, outside
    edits, unsafe changes caught, and the extra tokens.
  - [ ] 11. Agent extras, optional, never core (owner agreed 2026-11-15; owner rule: no forced model, agent-only
    features optional): an agent's own review tools can be used by that agent's steps when present (for Claude Code:
    the /security-review command and the official claude-security plugin, which does nothing until it is called and
    asks to confirm time and token cost). Qanuvi suggests one only when it helps (a project with sensitive paths
    before a release, a plan that touched them), asking once in words; never installs by itself. On a yes it installs
    for that one project only, in the per-project local scope so nothing is committed to the repo (confirm how the
    agent's scopes behave when building this); runs it at the release gate ("scan the changes since the last
    release") and feeds its findings into the same fix rounds; the project settings list it with Uninstall; an extra
    unused for a long time is offered for removal. Works only when that agent runs the step (on the user's own plan
    or key); free models and other agents get the checks above, which must always work without any extra.
- [ ] **24y. BENCHMARK for the plan runner: a long, complex plan that runs by itself (owner, 2026-11-14; a goal to
  reach, NOT a build step yet)**: the yardstick is 24z step A (R1 to R5) itself: about 25 steps across Rust and
  TypeScript, tests and test builds, a live check, a VPS check, all without the owner except the final merge and
  publishing. The owner decided R1 to R5 is built by hand this time because the runner is not mature enough yet; the
  runner is ready when a plan like it runs to the end alone. What the runner lacks today (from the 2026-11-14
  discussion):
  - Standing answers per plan: a policy section (allowed: commit and push to the plan's branch, test builds on
    scratch projects, named downloads; never: publish, certain folders) that the pre-run check and every ask read
    first, so covered questions are decided by Qanuvi, not sent to the owner.
  - A plan-level command allowlist (patterns, project-scoped: cargo test, npm run verify, builds, git on the plan's
    branch) with a fixed never-list (deletes outside the project, publishing, credentials); risky commands asking
    every time stops unattended runs today.
  - A branch per plan, a commit per step; a failed step is rolled back by itself and set aside; main untouched until
    the owner merges.
  - Check commands per step on its Done line (besides the test command), with fix rounds.
  - Test builds of the app: built into their own folder, never over the installed app, started with a separate data
    folder on a copied scratch project, checked through a test-only core command (no mouse), closed after; host fit
    check and memory guard built in.
  - Checks that need eyes go to one review list in the Inbox instead of holding the run.
  - Health watch: the proxy or a provider down pauses the run and notifies; it goes on when things are back.
  - A report at the end: done, set aside and why, waiting for the owner.
  Build these into the Rust runner (after R2), then run a plan of R1 to R5's size as the benchmark.
- [ ] **24v. Qanuvi's own context compression, for any AI, no Python (owner, 2026-11-14; agreed 2026-11-14; later: owner said
  it can wait while bigger steps toward a developer-friendly assistant go first; start only when the owner asks)**: the
  owner runs Headroom (headroom-ai 0.42, Python, Apache-2.0) in Qanuvi's terminal as a proxy at 127.0.0.1:8787 for
  Claude Code; measured 2026-11-14: 4 python processes, 544 MB (the proxy worker alone 513 MB), all counted in
  Qanuvi's status-bar total because it was started from Qanuvi's terminal. If that terminal closes, every Claude
  Code session (and plan steps) fails, because ~/.claude/settings.json points at it. What Headroom does: compresses
  only the new parts of a request (fresh tool output, latest turn) so the cached prefix stays byte-identical;
  routes by content type (logs: repeats dropped, errors/FATAL kept; JSON arrays crushed; code by syntax tree;
  diffs, search results, HTML trimmed; prose through its ML model Kompress via ONNX); keeps originals locally with
  a retrieve tool for the model; appends a "be terse" note at the end of the system prompt. Most of its memory is
  Python plus the ML parts. Qanuvi does it in Rust, own code (Headroom's ideas studied, its code not copied):
  - Part 1, inside Qanuvi's own agent (llm.rs, every provider: OpenRouter, OmniRoute, Ollama, paid APIs): a
    content router and compressors for tool results before they are sent, live part only, never the cached prefix;
    errors and failing lines always kept byte for byte; originals kept in the data folder with a
    `qanuvi_retrieve` tool (MCP, like the index tools); savings logged per step. No new process.
  - Part 2, outside agents (Claude Code, Codex, any OpenAI- or Anthropic-style API): a small Rust proxy inside
    qanuvi.exe (or its own small binary), started per run or session and handed to that agent through the existing
    "agent address" setting / `ANTHROPIC_BASE_URL` (OpenAI base URL for others), never the global settings.json; it
    ends with its owner (job object), so a closed terminal cannot break other sessions. Streaming passes through.
  - Part 3, later and optional: prose compression with a small model (ONNX from Rust, or the local Ollama model),
    only if the machine has room (machine::room/fits) and only if 24w shows heuristics are not enough.
  - Aim: tens of MB, not hundreds (estimate, to be measured). Headroom stays the owner's setup until part 2 works
    and 24w shows Qanuvi is close enough. On a Max plan the saving is room in the 5-hour window, not dollars.
- [ ] **24w. Keep measuring Qanuvi's compression against Headroom (owner, 2026-11-14; repeats, not one-off; later, starts with 24v)**: Headroom
  has been tuned for a long time, so 24v starts behind it. After each 24v part and after each tuning change: run the
  same scratch plan (same model, fresh plan file name) once through Headroom and once through Qanuvi's compressor;
  record input tokens, cache reads, output tokens, steps passed, retrieve calls, memory and time per request, in a
  table kept here (date, Qanuvi version, Headroom version, results). Look at what Headroom saved that Qanuvi did not
  (by content type) and turn the biggest gap into the next tuning task. Check Headroom's changelog for new tricks
  now and then. Done when Qanuvi is within about 10% of Headroom's saving with no drop in steps passed, and the
  owner agrees to switch Headroom off; until then this stays open.
- [ ] **24t. Chat clean-up (owner, 2026-11-13; agreed 2026-11-13)**: chats were never cleaned up by themselves:
  every session and all its lines (tool output too) stayed in the database until the owner deleted it, and 24j
  housekeeping covers plan files only. 24s part 2 assumed a "usual clean-up" that did not exist. One short session:
  - Settings, Agents, Chat clean-up: "Delete chats not opened for N days" (Off, 30, 60 or 90; default 60).
  - Never touches a starred chat (24s part 2), a plan-step session, a running or waiting session, or the chat open
    now. A chat counts as a whole: its handover chain goes together, judged by its newest session's last use.
  - Runs once at start-up (and from a "Clean up now" button), in Rust, in one transaction; events go with their
    sessions. Says once how many chats it removed ("Removed 12 chats not opened for 60 days"), nothing when none.
  - Then VACUUM only when much was removed (database file shrinks; skipped when small).
  - Built 2026-11-13, committed, NOT published: agent::clean_up_chats (Rust; chain grouped by continues_from, judged
    by its newest part's updated_at; plan-step sessions found by "sessionId" in plan_runs records; idle sessions kept
    too, since their process is alive; VACUUM after 5,000+ lines removed), command agent_clean_up, setting
    chatCleanupDays (default 60), Settings card with "Clean up now", start-up call in the main window only (keeps
    the chat open now and the session each project's workspace reopens), one inbox line "Removed N chats ...".
    Rust test in sessions_are_stored_renamed_listed_and_deleted.
  - Driven live 2026-11-14 (test build; test DB backed up first; six sessions set 90 days back): at start-up the
    plain chat went; kept as designed: the starred plan-step chat, a plan step, a handover chain whose newest part
    is recent, and scratch-project's only chat (the session its saved workspace reopens). Inbox: "Removed 1 chat
    not opened for 60 days". "Clean up now" afterwards: "Nothing to remove." No fixes needed.
- [ ] **24s. Handover in plain chat, bookmarks, reopening from the note (owner, 2026-11-13; agreed 2026-11-13,
  lean form chosen over a container layer)**: in plain chat the context grows until each tool call re-reads 150k+
  tokens. A separate container table under every chat was weighed and left out: edit and resend, rewind, branches,
  find, file history, cost totals, remote access and plan runs all assume one chat = one session, and reworking them
  buys nothing visible. The same result comes from linked sessions. About 3 short sessions:
  - (1) Handover in chat (every chat, bookmarked or not): each answer's context size is read (no extra cost; the 24p
    parts handoverDue and handoverRequest). Past the limit the agent finishes the edit in hand, writes the handover
    note (goal, done and checked, left, files and why, decisions, facts learned); a new session starts with the note
    and the last request and gets a "continues from" link to the old one (sessions column, migration). The chat
    shows the linked sessions as one timeline with a thin "New session · context carried over (was 152k)" line;
    the session features work on the current session, older parts read only. A message sent during the handover
    waits (24r area) and goes to the new session. Settings, Agents, Chat handover: When = Ask me first (default:
    a bar "This chat is heavy, hand over now?" / Not now), Automatic, Off; Limit = 150k (0 = off); a chat can turn it
    off for itself. Plan steps keep their own limit (Run panel, 200k). Every agent (Claude Code, Qanuvi's own, free
    models). Check first whether Claude Code's own /compact works in the stream-json session; Qanuvi's note stays
    the main way (other agents need it).
  - Built (part 1, 2026-11-13; not yet driven in the window): migration 5 (sessions.continues_from, handover_off;
    agent::link, set_handover_off, commands agent_link / agent_handover_off, also in master.rs). agent/handover.ts:
    chatHandoverDue (Not now holds back until 50k more), chatHandoverRequest (the agent answers with a note starting
    "Handover note"), chatResumePrompt / parseResume, noteAfter (falls back to the conversation's end), chainOf,
    chatsOnly. events.ts shows the request as a "Handing over…" line and the new session's first message as "New
    session · context carried over (was 152k)" with the note folded under it. stores/agent: context read from each
    assistant answer's usage; Ask shows the bar (Hand over now / Not now / Never in this chat), Automatic starts at
    once; the turn that answered ends, then a new session starts in the same chat (same agent, model, mode, title,
    linked); no answer in 3 min: stopped, the conversation's end stands in. Messages sent meanwhile wait ("goes to
    the new session") and are sent to it. select() loads the whole chain, older parts marked older (no edit,
    rewind or waiting area); the sidebar lists only the newest session of a chat; deleting a chat deletes its chain.
    Settings, Agents, Chat handover card (When, Limit in thousands). Plan sessions skipped (runner has 24p). Not
    done: the /compact probe (headless probes are blocked on this PC). Verify green (289 + 156 tests).
  - Live check (2026-11-13, same build, limit 20k; Claude Code starts at about 18.5k of context): the bar shows past
    the limit; Hand over now: the agent writes the note, the new session starts with the folded "New session · context
    carried over (was 20k) · the note" line, checks the note against the files, and a message sent during the
    handover goes to the new session and is answered; after a restart the chat reopens with its whole chain, and the
    sessions list shows it once (three linked sessions). Fixed from the check: (1) the new session started above a low
    limit (its own start plus the note) and offered a handover again at once, a loop: it now counts from its first
    measured size (offered again only after 50k more); (2) Settings search did not find "handover"; (3) the Limit field
    had the browser's light style. Verify green (291 + 156 tests).
  - (2) Bookmarks: a star on the chat and in the session list; starred chats in a "Saved" group on top (rename,
    unstar, search); housekeeping never deletes them; unbookmarked ones follow the usual clean-up.
  - Built (part 2, 2026-11-13; not yet driven in the window): migration 6 (sessions.saved; agent::set_saved, command
    agent_saved, also in master.rs). agent::delete refuses a starred session ("Unstar it first"), so nothing deletes a
    starred chat; there is no automatic chat clean-up yet, so "usual clean-up" is the owner's own Delete. agent::link
    moves the star to the new session at a handover (the newest session stands for the chat; the older parts are
    deleted with it). Sidebar: a star on each row (shown on hover, always shown in yellow when on; right-click Star /
    Unstar), Saved group on top and Recent below (handover.ts chatGroups), a "Find a chat" box once there are more
    than 6 chats (every word of the title, any case; Esc clears); a starred row has no Delete. Chat footer: a star
    beside the mode picker (not on plan steps). Rename stays as it was (right-click).
  - (3) Reopening a bookmark starts a fresh session from its last handover note, not a resume of the old heavy
    session; no note yet: written once at that moment (never on every leave: that spends tokens on chats never
    opened again). The note opens in a small panel, readable and editable; the next session works from the edited
    version.
  - Built (part 3, 2026-11-13; committed, NOT published, not driven in the window): opening a starred chat that is
    not live (not running, waiting or idle), not a plan step and at 40k+ context (handover.ts REOPEN_FRESH_FROM;
    below that a note costs about what it saves, so it resumes as before) shows a bar instead of the handover offer.
    No note yet: "Write the note now" (the old session writes it once; the note then shows in an editable panel) or
    just send (the note is written then, the message waits and goes to the new session). Note there: the panel shows
    it; edits are kept; the next message starts a new linked session from the edited note (no stale "last request").
    "Resume as it was" goes back to the old session. A written note is kept on this computer stamped with the
    session's line count (keptNote / keepNote), so it is reused only while the session has no newer lines. Opening
    a chat never spends tokens by itself (start-up restore shows the bar only). Store: agent.reopened, handOver(true)
    (noteOnly), #goOnFresh (shared with the handover), editReopenNote, resumeAsItWas.
    Follows Settings, Agents, Chat handover (owner, 2026-11-13): Automatic shows no bar, only a thin line, and the
    first message does it all; Ask me first shows the bar and note panel; Off (or "Never in this chat") resumes as
    it was.
- [ ] **24r. Messages sent while the agent works (owner, 2026-11-13; agreed 2026-11-13, built 2026-11-13)**: a
  message typed mid-work is written to the running Claude Code session at once, but the agent reads it only after
  the command in hand ends; a long command (a browser test run three times) can hold it for many minutes while the
  chat already shows it as part of the conversation.
  - (1) Keep it in a "waiting, read after the current step" area above the box until the agent has taken it in.
  - (2) A running timer on long commands (for example "Bash · 12 min").
  - (3) "Stop this command, read my message now": Claude Code's control request "interrupt" (what Esc does in its
    own terminal) ends the turn and its command, keeps the session, and the waiting message is read next. In a plan
    run the runner treats it as paused by the user, not a failed try. Later, optional: end only the command's own
    process (must not hit the MCP helpers that are also Claude Code's children). Qanuvi's own agent: simpler, it runs
    the commands itself.
  - Built: events.ts takeIn marks a message sent mid-work "waiting" until a tool result or the turn's end passes and
    the agent answers; it then moves to where it was read. AgentPane shows waiting messages above the box (not in the
    timeline) with "Stop this command, read my message now" (Claude Code only; Qanuvi's own agent reads between its
    steps). agent.rs interrupt() sends the interrupt control request; the turn's result after it is stored after a
    qanuvi_interrupted line (running steps show "Stopped by you", no error line, status idle not failed); an
    answer while idle sets the session running again (the queued message's turn). Runner: an interrupt on a step does
    not end the step's turn; nothing goes on within 30 s: paused by the owner (session kept for Resume). StepCard: a
    running step shows "· 42 s" / "· 12 min" after 10 s.
  - Live check (2026-11-13, release build of 35e7d13, scratch project, Claude Code on Sonnet 5.5): the timer counts
    on the running Bash step; a message sent during `sh slow.sh` waits above the box with the Stop button and, without
    Stop, moves in after the step ends and is answered ("Added while it worked"); Stop ends the command at once, keeps
    the session, and the next turn reads the message. Claude Code reads a queued message right after the tool result
    (the timing the headless probe could not see). Fixed from the check: the stopped step showed a red cross and
    Claude Code's raw "The user doesn't want to proceed…" text and the turn said "Finished": now the step has a stop
    icon and "Stopped by you" (either order of its result and Qanuvi's notice), the turn reads "Stopped by you · 26 s"
    in a neutral colour (events.ts state "stopped", result.stopped; rewind and session tools skip stopped edits).
  - [ ] 24r2 LATER (owner agreed 2026-11-15): say why waiting messages wait. Seen live 2026-11-15: the agent wrote a
    366-line file in one answer (about 5 minutes); the Waiting box showed both messages with "Working…" and nothing
    else, so it looked stuck, though it was not (Claude Code reads queued messages only after the tool result; they
    went in at the end of the write). While an answer streams one long tool call, the Waiting box says what it is and
    for how long, for example "the agent is writing a long file (2 min); it reads your messages when that is done",
    and points at "Stop this command, read my message now" for the urgent case. Uses the streamed tool input already
    shown in the step card (no extra reads); only after about 30 s, so short steps stay quiet.
- [ ] **24q. No token bursts: smaller steps and pacing (owner, 2026-11-13)**: measured on PLAN-D (3 days, about 45
  steps, $218 at API prices): the cost is cache reads, not new input. Each tool call re-reads the whole context, so
  a step of about 180 turns at 100-150k context reads about 20M tokens (88b 20.7M / $8.53, 87c 19.0M, 93z 21.8M,
  92c 18.8M). Fable 5.2 cost about the same per step as Opus 5.5 but about 1.1-1.8 times more per token (2 steps
  only); the owner moved runs to Opus on purpose.
  - (a) Smaller steps (agreed): the planner aims at one feature and at most 4-5 files per step; a step still too big
    is split before it runs (replan), so no step runs 150+ turns.
  - (b) Pace (agreed, optional, Claude paid plans only): a setting such as "stop plan runs at N% of my 5-hour
    window"; the run waits for the reset instead of using the window up, leaving room for the user's own work.
  - (c) Browser steps light (optional): screenshots off by default, only the changed pages checked.
  - Not done (owner): docs steps are NOT forced onto a cheap model; the user already picks models per step and not
    every user runs Claude. The Run panel's handover limit (200k) and step cost/minute limits stay the user's choice.
- [ ] **24n. Chat view and abilities at the level of the best agent panels (owner, 2026-11-12, later; a big step)**:
  the conversation should read and work like a first-class agent panel, for every agent (Claude Code, Qanuvi's own,
  free models). Parts, to be planned in detail before building:
  - Steps as cards: tool name and its description on top; IN (the command or input) and OUT (the result) in mono
    blocks, a few lines shown with a fade and click to expand; shell commands open by default, others on a click;
    copy buttons; the file a step touched opens from it. The test browser's steps (navigate, click, type, screenshot)
    show the address or element and the screenshot inline (15n images are the start).
  - A timeline: a dot per step on a thin line, the time on each message, steps that belong together grouped
    (several reads in a row become "Read 5 files"), long runs folded.
  - Edits as diffs inside the chat (added and removed lines, per file, with Open and Revert), not only the summary.
  - Thinking shown folded; plans and to-do lists the agent writes shown as a checklist that updates live.
  - The box: images by paste (done), drop files and images onto it, @ mentions of files and symbols with a picker,
    / commands, the model, mode and effort in reach, a queue of messages while it works (15j), and stop.
  - Session abilities: edit and resend an earlier message (branching from there), rewind changes to a step, search
    in the conversation, export, and a compact "what changed" summary at the end of each task.
    Done 2026-11-12 (part 6: edit and resend, rewind, search), committed, NOT published: on hover (or right-click) a
    message of the owner offers Edit and Rewind. Edit opens the message in place (Enter sends, Esc cancels) and
    "Send as a branch" starts a new session on the same agent, model, mode and effort, titled "... (branch)", told
    what was said before (owner and agent text, a line per step, the newest 24k characters) and whether the files
    were reset; the original session stays as it was. This works the same for every agent (no agent-side fork
    needed). A checkbox "Also undo the file changes made from here on (N steps in K files)" rewinds first. Rewind
    undoes the Edit/MultiEdit steps from that message on, newest first, per file, after a preview of the files;
    a file changed since, or written whole by a step, is left as it is and named; failed steps are skipped. Search:
    Ctrl+F in the pane (or the footer's search button): a bar with "n of m", Enter / Shift+Enter, Esc; groups and
    folds holding a hit open, step cards with a hit show all, words marked with the CSS Highlight API. Code:
    agent/sessionTools.ts (+ tests), agent.branch, AgentPane. Export, the "what changed" summary and the virtual list
    stay open. Not driven in the running app yet.
  - Fast with long sessions (virtual list) and light on memory, as the rest of Qanuvi.
  - The aim (owner, 2026-11-12): chatting with a paid agent directly (not the task runner) should feel like working in
    the best agent panels: the same agent and model, so the gap is mostly what the user can see and steer.
- [x] **24o. A finishing pass at the end of a plan run (owner, 2026-11-12, later)**: the steps follow their
  instructions; before the run is called done, one smarter pass checks the whole. The final report offers it with its
  estimated cost and time (from the run's own step costs); the user runs it or ignores it (a project setting can run
  it by itself). Light: the project's full checks and a review of the whole run's diff (leftovers, debug code, steps
  that do not fit together). Full: also live tests on a scratch copy where the project defines them, a fix-up of what
  was found, and the task list notes checked against what was built. It ends with a short "still open" list. Runs on
  the paid decider (24e) while free or cheap workers do the steps: intelligence paid for once, where it catches most.
  Done 2026-11-12, committed, NOT published (ask the owner first): a run that ends done with work in it shows a
  "Finishing pass" card in the Run panel with Light and Full buttons and an estimate from the run's own finished steps
  (light about half a typical paid step, full 1.5 steps growing with the run, at most 4; "cost not known yet" when only
  free steps finished). A plan's run setting "At the end, the finishing pass" offers it (default) or runs light or
  full by itself. Light: the project's test command, then the paid decider (agent.runProviderId, the planner model)
  reviews the run's whole diff (a snapshot is taken at the run's start, so earlier uncommitted work is left out; after
  a restart the changed files' diffs against the commit; the plan, its archive and logs left out) with the Done notes;
  free models review if the paid agent cannot answer, and the report says so. Full: also a session on the decider that
  fixes what was found, removes leftovers, checks Done notes against the code (corrects a note, never re-marks steps),
  runs live tests only where the project or plan defines them and only on scratch copies, then the full checks with
  Qanuvi's fix rounds; it ends with a "Still open:" list read from its answer. The card shows findings, still open,
  cost, time and checks; Stop works; "Open its session". Its cost counts in the run (its own line), an entry goes to
  the plan's FINISH.md log and the inbox. Also folded in: the planner and the rewrite of a failed step number new
  steps after the plan's archive too; the chat footer says "switching after the edit in hand" while a picked model
  waits. Code: plan/finishing.ts (+ tests), runner finishPass. Not driven live yet: try it on the scratch plan.
- [ ] **M3d. Nested masters at depth 3 and the loop guard, driven live (owner, 2026-11-17, later: when the owner
  arranges another machine)**: M3c is built and driven at depth 2 (window -> local test master -> VPS). Drive: window ->
  local test master -> VPS -> the new machine (calls, terminal, preview through three hops); a path back to a machine
  already passed is refused with a plain message (loop); a fourth level is refused (depth). Also not driven yet: a
  master disconnected on purpose while one under it is shown; unpair (removed) live; a language server on a nested
  machine (unit-tested). The VPS already runs a server build that forwards (d57f56b).
- [ ] **24k. Master OmniRoute (owner, 2026-11-12, later)**: OmniRoute is said to reach about 300 models with about 1.6
  billion free tokens a month. Learn its providers, limits and sign-ins; automate as much of the setup as possible
  (connections, keys, combos, health); keep what is learnt in Qanuvi's brain (the signed remote knowledge, 24c) so
  every Qanuvi knows it. Headroom can sit in front of it for the free models too (`--openai-api-url`), to be tested.
- [ ] **24k2. Free Claude Code (FCC) as a second free provider (owner, 2026-11-14, later)**: github.com/alishahryar1/
  free-claude-code, a local router (127.0.0.1:8082, own Admin page, Windows tray app) to 61 providers with free tiers,
  about 1.3 billion free tokens a month. For people who cannot pay: weaker models still give real help (questions,
  explanations, simple edits, plans); Qanuvi offers it and says plainly what to expect, never pushes it.
  - What it adds next to OmniRoute: it can stand in for Anthropic's server, so the Claude Code agent Qanuvi drives
    can run plan steps on free models (today those steps are paid only). For Qanuvi's own agent it is an
    OpenAI-compatible address (/v1) like OmniRoute, so an alternative there, not something new.
  - Scope: a setup guide like OmniRoute's (guides.ts, setup wizard); run Claude Code steps through FCC per plan step
    or per project (base URL set only for that agent process); the step label says it runs on a free model; a
    check that FCC is running (and a clear note when it is not); its models listed for the Pick models button.
  - Watch: needs Python 3.14 through uv (its installer brings it); AGPL v3, so only talked to as a separate
    program, none of its code copied in; the owner's Claude Code already goes through Headroom (8787) and FCC has its
    own Headroom option, so the order must be clear; some free tiers keep or train on what is sent (say so in the
    guide); free models are weaker (owner: code quality over token saving), so suggested for light steps only.
  - First: try it on a scratch project and compare free models with Haiku 5.5 on the same small plan.
- [x] **1f. Qanuvi's own resource use (done 2026-11-12)**: the status bar shows Qanuvi's total memory and CPU (qanuvi.exe, its
  WebView2 processes and the agents it started); Task Manager lists WebView2 apart (1e).
- [ ] **15e3. Temp files follow the data folder (owner, 2026-11-12, later)**: measured on the owner's PC, Temp on C:
  held about 0.9 GB of Claude Code working files plus Qanuvi leftovers (test browser profiles 58 MB, old updater
  downloads 42 MB, test folders 21 MB). To do: start agents, plan runs, checks and the test browser with TMP/TEMP set
  to a temp folder inside Qanuvi's data folder (movable, 15e), clean it at start-up (left-over browser profiles,
  updater downloads after a finished update), and show its size on the Servers/data page with a Clean button.
- [ ] **1g. Status bar memory: the app apart from agents (owner, 2026-11-12, later)**: the status bar showed 835 MB,
  which looked like Qanuvi alone. Measured: Qanuvi + WebView2 about 283 MB, a Claude Code agent 507 MB, a terminal
  PowerShell 56 MB. To do: show the app's own memory in the status bar, with a hover breakdown of app (qanuvi.exe +
  WebView2), each agent and terminals, plus the total.
- [x] **24j. Plan housekeeping**: move finished sections to an archive file next to the plan, and add another
  plan's steps into the current one, both with a preview and the owner's confirmation.
  Done 2026-11-12: two buttons in the Plan view header (Archive, Add steps), each opening a preview panel; nothing is
  written before its button is pressed, and the change is worked out again from the files as they are then. Archive:
  every section whose steps are all done (heading, steps and text under it) goes to PLAN.archive.md next to the plan
  (made or added to; written first, so a failure never loses steps). Plan runs read the archive: its done steps
  satisfy Needs (also "OTHER.md#n" needs, from that plan's archive) and clear stale Skipped lines. Archives are not
  offered as task lists. Add steps: pick another task list, "Only steps not done" (on by default); its sections go to
  the end under their headings, numbers in use here or in the archive get the next free ones ("45e" keeps its
  letter), Needs lines follow (renumbered; a need on a left-out step becomes "OTHER.md#n"; "THIS.md#n" becomes "n");
  the source file stays as it is. Both buttons are off while a run of this plan is going. The agent brief mentions the
  archive. Code: plan/housekeeping.ts (+ tests), HousekeepingPanel.svelte. Not yet driven in the running app.
- [x] **15k. Search like VS Code (owner, 2026-11-12)**: the search panel found 0 results for a word that
  is in the project. To do: results grouped per file with the line and the match highlighted, case, whole word and
  regex toggles, files to include and exclude, gitignore respected, results while typing, replace and replace all,
  click to open at the line; fast on large projects (search in Rust, streamed).
  Done: results while typing, grouped per file with every match marked and a count; match case, whole word and
  regular expression; files to include and leave out (globs like "src/**, *.php", "vendor"); replace in one file or
  all (with a confirmation; open files are read again); click opens at the line and column. Search runs in Rust.
- [x] **24g. Qanuvi introduces itself to every model (owner, 2026-11-12)**: an agent asked for "a Qanuvi
  plan" had to dig through Qanuvi's source to learn what one is. To do: every session starts with a short brief from
  Qanuvi core: what Qanuvi is, the plan and task list format (sections, steps, needs, model per step) with an example,
  where plans live and how the runner runs them, the Inbox, memory, and what the agent may ask Qanuvi to do. Kept
  short (cached), one copy for all agents, updated with the app (and by signed remote knowledge, 24c).
  Done: src-tauri/src/brief.rs (the plan format the parser reads, statuses, Needs, Model, where plans live); Claude
  Code gets it with --append-system-prompt (not in lean plan-step sessions), Qanuvi's own agent in its system
  prompt. Live: a fresh session asked only to make a plan wrote PLAN.md in the right format without reading any of
  Qanuvi's code, for $0.11 (the same kind of request cost $3.86 before). Still to add: the Inbox, memory and runner
  commands, and updates by remote knowledge.
- [ ] **1e. Task Manager lists WebView2 apart from Qanuvi (owner, 2026-11-11, later)**: the WebView2 processes are
  Qanuvi's children (checked), but Task Manager shows them under "WebView2 Manager". Try giving the window and its
  processes Qanuvi's app id so Windows groups them; cosmetic.
- [ ] **1b. Updates: update channels, signed updates and a smooth update popup** (added 2026-11-10; moved by the owner on 2026-11-10 to after the app shell is usable, so it is built after Milestone 8; moved later again by the owner on 2026-11-10 while other milestones go first; originally planned right after
  Milestone 1 because the app will be updated very often during development)
  - Tauri updater plugin: the app checks an update address for a small manifest (`latest.json`: version, notes, date,
    download address and signature per platform), downloads in the background, installs on restart.
  - Channels: `dev` (now), `beta` and `stable` (later); the channel and its address are a setting.
  - The `dev` channel for now is a local folder served by the owner's web server:
    `E:\Wnmp\nginx-bins\default\www\qanuvi-updates\dev\` (for example `http://localhost/qanuvi-updates/dev/latest.json`;
    the exact address is set when the folder is created). Plain `http` is allowed only for this local channel; `beta`
    and `stable` must use `https`.
  - Signed updates: a key pair made with the Tauri signer; the public key is in the app, the private key stays outside
    the repository (never committed), and an update with a wrong signature is refused.
  - Popup for permission: "Qanuvi 0.x.y is ready", the release notes, **Update now** / **Later** / **Skip this version**;
    an option to download automatically and only ask before restarting.
  - Smooth: the update waits while an agent is running a step and installs between steps; layout, open projects, tabs
    and sessions are restored after the restart; if the new version fails to start, the previous one is kept.
  - Check on start and every few hours (a setting), and "Check for updates" in the menu.
  - Publish script (one command): build, sign, write `latest.json`, copy the files to the channel folder.
  - Validate: install version 0.1.0, publish 0.1.1 to the local channel, the popup appears, update and restart restore
    the layout; a wrongly signed file is refused; "Later" and "Skip" behave; an update offered during a running step
    waits for the step to end.

  - Decided by the owner (2026-11-11), built next:
    - Installer (option 1): the NSIS installer branded (Qanuvi logo and colours on the side and header images, the Q
      icon on the setup file), fewer steps (welcome, an optional folder, progress, finish with "Launch Qanuvi"),
      installed for the current Windows user (no admin prompt), Start menu and desktop shortcuts. Updates use the
      same installer silently. A fully custom one-click installer is for later, before a public release.
    - No admin rights for the app: it runs as the user. A step that changes the system (installing a program, system
      settings, services) asks in Qanuvi's approval card first, then Windows asks for admin for that one step.
    - Update flow: download in the background with a thin progress bar in the status bar; then "Qanuvi 0.x.y is
      ready - Restart now / Later" with what is new; on Restart now: close, install silently, reopen where the user
      was. The dev channel is served at https://qlopy.top/qanuvi-updates/dev/ (latest.json, installer, signature),
      written by one publish command.
    - Reload window (Ctrl+Shift+R and in the command list): the interface reloads while sessions, terminals and agents
      keep running and reconnect. Restart Qanuvi: a full restart (asks first when an agent is working).
- [-] **2. Layout: sidebar, agent pane, tabbed work pane, terminal panel** (built 2026-11-10, waiting for the owner's
  review)
  - Three panes as in the design: sidebar (left), agent pane (middle), work pane with tabs (right); the terminal panel
    split under the editor like VS Code.
  - Drag to resize, hide and show each pane (with shortcuts), collapse the sidebar to icons, maximise the terminal.
  - Named layouts saved and switched ("Coding", "Side screen", "Review"); the last layout is restored on start.
  - Validate: every pane resizes, hides and comes back; layouts survive a restart; the design matches the reference feel.
  - Built: `src/lib/layout/layout.ts` (rules: limits, defaults, the three presets, cleaning of saved data, at most 20
    named layouts, the window never left empty) and `layout.svelte.ts` (live state, saved to local storage a quarter
    second after the last change); components `TitleBar` (Qanuvi's own bar on Windows with the layout menu, pane
    toggles and window buttons; macOS keeps its window buttons over the bar; Linux keeps the system title bar),
    `LayoutMenu` (switch, save as, remove; a removed preset goes back to its original settings), `Sidebar` (workspace,
    Home, Inbox, Projects, Plans, Agents, Reminders, Projects and Sessions sections, collapse to icons), `AgentPane`
    (empty state and the "Ask Qanuvi…" box), `WorkPane` (Welcome/editor, Preview and Diff tabs; terminal panel under
    them with maximise and hide), `Splitter` (pointer drag and arrow keys) and `shortcuts.ts`: Ctrl+B sidebar, Ctrl+J
    terminal, Ctrl+Shift+J maximise terminal, Ctrl+Alt+A agent pane, Ctrl+Alt+W work pane (Cmd on macOS). Icons from
    Lucide, imported one by one. Window config per system: `tauri.conf.json` (no system frame), `tauri.macos.conf.json`,
    `tauri.linux.conf.json`; window permissions in `capabilities/default.json`.
  - Checked on Windows: `npm run verify` green (18 frontend tests: layout rules and shortcuts; 2 Rust tests; clippy and
    svelte-check without warnings); screenshots of the full layout, Ctrl+B and Ctrl+J hiding the sidebar and terminal,
    the layout coming back the same after closing and reopening, Ctrl+Shift+J maximising the terminal and restoring it.
    Memory with the full layout, idle: 84.4 MB private in 7 processes, CPU 0%.
  - Not yet checked by hand (the owner's review): dragging the splitters with the mouse, the layout menu (switch, save,
    remove), the window buttons and dragging the window by the title bar; macOS and Linux title bars.

- [x] **3. Projects** (done 2026-11-10)
  - Add a project folder; detect its type from its files (`composer.json`, `artisan`, `pubspec.yaml`, `*.csproj`,
    `requirements.txt` / `pyproject.toml`, `build.gradle`, `package.json`) and show it with a coloured icon.
  - Project list in the sidebar, switching projects, remove from the list (never deletes files), per-project settings
    (run, test and preview commands, preview address).
  - Validate: the owner's projects (for example the qCommerce plugin and a Laravel or Flutter project) are detected
    correctly.
  - Built: SQLite store `src-tauri/src/store.rs` (app data folder, `qanuvi.db`, versioned migrations; tables projects,
    sessions, session_events), `projects.rs` (detection of Qlopy, Laravel, Flutter, Android, .NET, Python, Rust, Node,
    PHP, plain folder; all tags kept; default run/test/preview commands per type, `php tests/run.php` found for
    qCommerce-style plugins; add, list, update, remove, look again), sidebar project list with a coloured type badge,
    settings and remove buttons, `ProjectSettings` dialog (name, run, test, preview address, plus "tabs follow the
    project" and terminal shell for all projects), folder picker (dialog plugin). Added beyond the plan: `qanuvi <folder>`
    (or `qanuvi .`) opens a folder as the current project, like `code .`.
  - Validated: 6 Rust tests (each type, tags, defaults, Flutter not also Android, add twice keeps one entry, update,
    remove leaves files, refuses missing folders and files, Windows `\\?\` prefix removed). In the app: `qanuvi .` in
    `E:\Projects\qanuvi` and in a test project; both listed and switchable.
  - Not checked: the owner's Laravel, Flutter and qCommerce folders in the app (the rules are covered by the tests); the
    folder picker by hand (it opens; choosing a folder in it was not automated).

- [x] **4. File tree and code editor** (done 2026-11-10)
  - File tree for the project (respects `.gitignore`, large folders loaded lazily), editor tabs, CodeMirror 6 with
    highlighting for PHP, JS/TS, CSS, HTML, Markdown, Python, Dart, C#, Rust, JSON, YAML, SQL.
  - Save, unsaved marks, search in the file, search across the project (Rust, ripgrep-style, results in a panel),
    go to file.
  - Setting: editor tabs follow the project (switching project swaps the open tabs) or stay open across projects.
  - Validate: open and edit large files smoothly, project search on a large codebase, memory recorded.
  - Built: `files.rs` (every path checked to stay inside the project, links included; folder listing and file list with
    the `ignore` crate, so `.gitignore`, global ignores and `.git` are respected; read with binary and 10 MB checks;
    save through a temporary file then rename, refused if the file changed on disk since it was opened; regex or plain
    search, case option, 2 MB per file, 2,000 results). Interface: `Explorer` (lazy tree, refresh, collapse), editor
    tabs with unsaved dot, middle-click to close, Ctrl+W, close asks before losing changes; one CodeMirror view with a
    state per tab (text, cursor, undo kept when switching); 26 languages loaded only when first needed
    (`editor/languages.ts`, legacy modes for Dart, C#, Kotlin, Java, C/C++, shell, PowerShell, TOML, Dockerfile, Groovy,
    nginx); dark theme from the design tokens; status line (path, unsaved, line and column, language); "changed on
    disk" bar with overwrite or reload; Ctrl+S, Ctrl+F (in-file search and replace), Ctrl+P go to file (fuzzy),
    Ctrl+Shift+F project search panel; "unsaved" clears when undo returns to the saved text.
  - Validated: 6 Rust tests (paths outside refused, listing order and ignores, file list, binary detection, save and
    the on-disk conflict, search options and limits) and frontend tests for languages and fuzzy matching. In the app on
    `E:\Projects\qanuvi`: tree, TypeScript highlighting and folding, Ctrl+P "tasklist" opened `tasklist.ts`, project
    search "fuzzyScore" gave 6 results in 2 files (77 searched).
  - Not checked: a very large file and a very large codebase (speed); the "tabs follow the project" switch by hand.

- [x] **5. Terminal panel** (done 2026-11-10)
  - Real terminals (PowerShell, cmd, Git Bash on Windows; bash or zsh elsewhere) in tabs under the editor; several per
    project, started in the project folder; resize, clear, kill, rename.
  - Validate: run the qCommerce test suite in it; long output stays smooth; closing a project closes its terminals.
  - Built: `terminal.rs` (portable-pty: ConPTY on Windows; shells found on the machine: PowerShell 7, Windows
    PowerShell, Command Prompt, Git Bash, or the login shell plus bash, zsh, fish elsewhere; output streamed over a
    channel with characters split between reads kept whole; resize; kill one, a project's, or all when the window
    closes). Interface: xterm.js with the Qanuvi colours, fit to the panel, tabs per project that keep running while
    another project is shown, the tab title follows the shell, shell picker, Ctrl+C copies a selection and Ctrl+V
    pastes. No terminal is started until asked (a shell costs memory).
  - Validated: 3 Rust tests (split characters, invalid bytes, at least one shell). In the app: PowerShell opened in the
    project folder and `git status --short` printed its coloured output.
  - Not checked: the qCommerce test suite and very long output in it; rename (the title follows the shell instead);
    clear (use the shell's own `cls`/`clear`).

- [x] **6. Preview tab** (done 2026-11-10)
  - Address bar, reload, back and forward, device sizes (desktop, tablet, phone); uses the project's preview address.
  - Unloaded when its tab is hidden.
  - Validate: a local site loads in each size; memory with and without the preview recorded.
  - Built: `PreviewTab` (address bar that adds `http://` for local addresses and `https://` otherwise and refuses
    anything that is not a web address, own back/forward history, reload, desktop / 820 px tablet / 390 px phone
    frames, open in the browser); the frame exists only while the tab is shown; content security policy allows framing
    `http`/`https` pages only.
  - Validated: `url.test.ts`; `http://localhost/` (the owner's nginx) shown in the app. Memory with the preview, an
    editor tab and a session open: 104 MB for Qanuvi and its web view (the full layout without them: 84 MB).
  - Known limit: sites that forbid being framed (`X-Frame-Options`) stay blank; "open in the browser" is the way round.
    Back and forward follow the address bar, not links clicked inside the page.
  - Added after the owner's review (2026-11-10): "Fill the Qanuvi window" (the preview covers the whole app, with its
    address bar and an Exit button) and "Full screen" (the same, and the window goes full screen); Esc or Exit returns to
    the normal layout. Checked in the app. While the focus is inside the page, the page keeps the keyboard, so Esc
    reaches the page instead; the Exit button always works.

- [x] **7. Task list view** (done 2026-11-10)
  - Read the project's task list (for example `DEVELOPMENT-TASKLIST.md`): sections, numbered steps, status
    (`[ ]`, `[-]`, `[x]`, `[!]`), sub-points; shown as a plan in the sidebar and a list in the work pane.
  - Change a step's status and edit text from the view; the file stays readable Markdown and other edits in the file
    are kept.
  - Per-step settings stored in the file in a simple, readable form (for example the model for a step).
  - Validate: the qCommerce task list (Plan D) reads and writes back without changing anything else in the file.
  - Built: `plan/tasklist.ts` (sections, steps with number, title, wrapped note, details and a `- Model: <id>` line;
    edits change only the lines they must and keep CRLF or LF; an edit is refused if the step at that line no longer
    has the title that was shown); `PlanView` (task lists found by name: TASKLIST, TODO, PLAN, ROADMAP; picker when
    there are several; progress per section; All / Open / Done; click the status circle to cycle it; double-click a
    title to rename; model per step; details; "show in the file"); saving uses the on-disk check, so a file edited
    elsewhere is reloaded, not overwritten. "Plans" in the sidebar opens it.
  - Validated: 13 frontend tests (parsing, continued notes, details, model line, CRLF, one-line status change, rename,
    add/change/remove model, refusal on a changed file, file discovery). In the app: Qanuvi's own task list shown with
    stages, counts and notes; in a test project a click changed `- [ ]` to `- [-]` on disk and nothing else.
  - Not checked: the qCommerce task list in the app (its format is the one the tests use).
  - Added after the owner's review (2026-11-10): "Ask the agent to write one" (empty state) and a button in the Plan
    header open a new agent session with a ready-made request (`plan/prompts.ts`) to write or extend the task list in
    Qanuvi's format, with a suggested model per step (strongest for money, security and hard design, a mid model for
    normal coding, the smallest for small edits and docs); the user reads, changes and sends it. A test checks that the
    example in the request is a step Qanuvi reads, model included.

- [x] **8. Agent pane: run an agent with a chosen model** (done 2026-11-10)
  - Start an agent session (Claude Code first) in the project folder, with the model chosen for the session; stream its
    progress into the agent pane as quiet step lines and result cards; approval cards for what it asks; stop.
  - Clicking a result card opens the file, diff or page in the work pane. Diff tab with accept and revert.
  - Sessions listed in the sidebar with a status dot; tokens and time shown per session.
  - Validate: a real small task done end to end from the app on the owner's project; VS Code not needed for it.
  - Built: `agent.rs` runs `claude -p` with streamed JSON in and out, `--model`, `--permission-mode` (Ask before
    changes = `default`, Allow edits = `acceptEdits`, Plan only = `plan`; never "bypass") and
    `--permission-prompt-tool stdio`, so every permission request comes to Qanuvi and is answered allow or deny; the
    process stays open for follow-up messages and a finished session is resumed with `--resume`; every message is kept
    in `session_events` (with Qanuvi's own notes of the user's messages and answers) so a session reads back in full
    after a restart; tokens (input with cache writes, cache reads, output), cost and time are added up per session from
    each turn's result. Interface: `agent/events.ts` turns messages into the user's messages, the agent's text, step
    lines (running, done, failed, with the first line of the result; a step on a file opens it), approval cards with the
    command or the change shown and Approve / Refuse, a finished line with time; model and permission pickers; Stop;
    usage line; sessions in the sidebar with status dots (running, waiting, done, failed, ended), delete; the Inbox
    shows how many sessions wait for an answer. `git.rs` and `DiffView`: branch, changed files with status letters,
    the diff (untracked files shown as added), accept = stage, undo = restore from the last commit after a
    confirmation (new files are never deleted); refreshed when an agent turn ends.
  - Message format checked against the real CLI first (one test call on Haiku, $0.004).
  - Validated: 6 Rust tests (titles, result totals, sessions stored, renamed, listed, deleted with their events,
    removed with their project, history order, git status and diff on a real repository) and frontend tests for the
    message reading. In the app, on a test git project with Haiku 5.5 and "Ask before changes": the agent read
    `notes.txt`, asked to edit it, the approval card showed the change, Approve applied it (file on disk changed),
    finished in 42 s (including the wait) with "in 14k · cache 42k · out 429 · $0.0035"; the Diff tab then showed the
    change; after closing and reopening Qanuvi the session read back in full.
  - Not checked: Stop during a run; a follow-up message in the same session; Refuse from the app (the CLI side was
    checked in the format test); the agent on the owner's real project.
  - Added after the owner's review (2026-11-10): an **Auto** permission mode (the agent's own auto mode: routine actions
    go ahead, risky ones are stopped; "bypass all" is still never offered), and an **agent address** setting for a
    proxy such as Headroom, passed to each new session as `ANTHROPIC_BASE_URL` (empty: Claude Code's own setting
    applies); both in Settings > For all projects, with the default permission mode for new sessions. Checked: Rust test
    for the address rules and the modes; `claude -p --permission-mode auto` answered on Haiku. MCP servers added with
    `claude mcp add` (for example in Qanuvi's terminal) are used by Qanuvi's sessions, since they run the same CLI.

- [x] **8b. Agents as add-ons: Agents panel, agent config files, composer like a coding agent's** (added by the owner
  and done 2026-11-10)
  - Why: Qanuvi will run many agents (and agents from extensions), so nothing in Qanuvi's own interface may be written for
    one agent; an agent describes itself and Qanuvi shows what it describes. The owner asked for a composer that feels
    like a coding agent's (mode list with icons and descriptions, a "/" menu, the model shown under the box) and for a
    way to see the connected agent, edit its configuration and know what other agents are coming.
  - Built: `agents/providers.ts` (an agent describes its name, badge, models, permission modes with icon and
    description, effort levels, whether it can use a proxy address, and how to install it; Claude Code is the first,
    with Manual, Edit automatically, Plan and Auto, and efforts low to max); per-agent settings in `settings.svelte.ts`
    (model, mode, effort, address; older single settings carried over); Rust `agent_config.rs` (only a fixed list of each
    agent's files can be read or written: for Claude Code `~/.claude/settings.json`, `~/.claude/CLAUDE.md`, the
    project's `.claude/settings.json`, `.claude/settings.local.json`, `CLAUDE.md` and `.mcp.json`; a missing file opens
    empty and is created on save; JSON must be valid to be saved) and `--effort` passed to sessions. Interface:
    `AgentsView` (Agents in the sidebar or the gear under the message box: status and version, where it is installed,
    "use for new sessions", model, mode, effort, address, configuration files that open in the editor, and "Add an
    agent" with what is coming next); `ModePicker` (Shift+Tab cycles modes); `CommandMenu` ("/" button or typing "/" in
    an empty message: mention a file from the project, clear the conversation, export it as Markdown to the clipboard,
    switch model, effort slider); the agent's badge, model and effort under the message box; editor tabs for config
    files (global ones stay visible in every project) with a save-error bar that keeps the text; the project settings
    dialog no longer holds agent settings.
  - Validated: Rust tests for the config file list, reading a missing file, creating on save, invalid JSON refused,
    unknown ids refused, changes made elsewhere refused; frontend tests for agent settings and Markdown export;
    `npm run verify` green (51 frontend, 32 Rust tests). In the app: mode list and "/" menu open fully, Agents panel shows
    Claude Code connected (2.1.302) with its six files, `.mcp.json` opened in the editor and `{bad}` was refused with the
    JSON error and no file written.
  - Not checked: saving a real change to the owner's own Claude settings (left to the owner); Shift+Tab by keyboard in the
    app; "Mention a file" picking end to end.

- [x] **9. Model per step: run a task list with a different model per step** (done 2026-11-10)
  - Run a plan from the task list step by step; each step uses its own model (the step's own setting, the user's rules,
    a project default); switching between steps needs nobody at the screen.
  - Each step starts with a short fresh context (the step text, a summary of earlier steps, the relevant rules) instead
    of one ever-growing conversation.
  - A step that fails twice on a cheaper model moves up to a stronger one; the change is recorded on the step.
  - Tokens, cost, time and outcome recorded per step and per model; totals per plan.
  - Validate: a three-step plan where each step uses a different model runs unattended; the switches and token counts
    are visible; a forced failure moves a step up.
  - Built: `plan/runner.ts` (decisions, tested: steps to run, a step's first model = its `- Model:` line or the agent's
    default, next model after failures with models listed strongest first, a step's outcome read back from the file by
    number and title, the step's request, totals per model); `stores/runner.svelte.ts` (the run: one fresh agent
    session per attempt with only the step, its details, the steps already done in the run and the rules "work only on
    this step; mark it `[x]` with a Done line when checked, or `[!]` with the reason if only the user can unblock it";
    success is read from the task list after the turn; after the chosen number of failed tries (1 to 3, default 2) the
    step moves one model up; the run stops when the strongest model fails, a step is blocked, the budget is reached or
    the user presses Stop; each session is named after its step and closed when the step ends, to free memory; the
    session shows live in the agent pane); Rust `runs.rs` and database version 2 (`plan_runs`, one JSON record per run,
    cleaned up with its project; runs going when the app closed are marked stopped); `RunPanel` (Run button: start at a
    step or only one step, permissions (Auto by default for unattended work, with a warning for Manual), budget, failed
    tries before a stronger model; live banner with step, model, try and Stop; totals per model; summary when the run
    ends) and a chip per step with the model of its last try and its cost.
  - Validated: 8 frontend tests for the decisions and a Rust test for run records; `npm run verify` green (59 frontend,
    33 Rust tests). Real run on a test project (Auto, 1 try per model, budget $0.15): step 1 on Haiku done ($0.0061)
    and marked `[x]` with a Done line; Qanuvi switched to Sonnet for step 2 by itself, done ($0.10, list sorted); step 3
    (written to be left open) failed on Haiku, moved up to Sonnet, failed again, and the run stopped at the budget
    ("The run reached its budget of $0.15"); totals Haiku 2 tries $0.0085, Sonnet 2 tries $0.14.
  - Found: a fresh session per step pays the agent's start-up context each time (about 17–20k tokens written to the
    cache), so a tiny step on Sonnet still cost $0.10.
  - Fixed the same day with **lean sessions** (Run option, on by default): `--strict-mcp-config` and
    `--tools Read,Edit,Write,Bash,Glob,Grep`. Measured with one tiny request on Haiku: normal start-up 18,252 tokens;
    without MCP servers 17,507; lean 8,992; the same lean start-up again read 8,195 from the cache and cost a sixth
    (the agent keeps its cache for an hour, per model). Same three-step run as above, lean: step 1 Haiku $0.0046, step 2
    Sonnet $0.058, step 3 Haiku $0.0007 and Sonnet $0.0125 (both mostly from the cache): about $0.076 for what cost
    about $0.16 before (−52%); the budget then also covered a third try on Opus ($0.076). Running steps on the same model
    one after another keeps the cache warm; Milestone 13 can use that when choosing models.
  - Also fixed: a race where the Plan view, still loading the last run's record, replaced a run that had just started
    (the run stopped at once); the run now keeps its own reference and an old record never replaces a running one.
    Sessions of a run are named after their step ("Step 2: Sort the list", "(try 2)").
  - Still to try in Stage 2: `--bare` would cut more but needs an API key instead of a subscription sign-in.
  - Known limits: the budget is checked before each try, so a run can go over by at most one try; the run lives in the
    window (a headless runner is Milestone 16); "the user's rules" for choosing models come with Milestone 13.

- [ ] **10. Stage 1 review**
  - Memory and CPU table for every milestone; the owner uses Qanuvi instead of VS Code for a full working day; list of
    fixes before Stage 2.

## Stage 2: the brain and token saving

- [x] **11. Code index and cheap tools**: a code index (files, symbols, references) kept up to date, search and "give me
  this file" tools the agent can call, answered by the app without reading the whole project. (done 2026-11-10)
  - Built: Rust `index.rs` (definitions found with per-language patterns for PHP, JS/TS/Svelte/Vue, Rust, Python, Dart,
    C#, Go, Java/Kotlin: functions, methods with their class, classes, interfaces, traits, enums, structs, types,
    modules, constants; `.gitignore`, `node_modules` and `vendor` left out; files over 1 MB skipped; a refresh reads
    again only files whose size or time changed, and drops deleted files); `mcp.rs`: **`qanuvi --mcp <folder>`** runs
    Qanuvi as a read-only MCP server with no window, tools `project_map`, `find_symbol`, `outline`, `read_lines` (at
    most 300 lines, inside the project only) and `search`, each answering in a few lines of text; sessions get it
    through `--mcp-config` (a small config written per project under the app data folder) with
    `--allowedTools mcp__qanuvi`, so the read-only tools never ask for approval; "Code index tools" option per agent
    (Agents panel, on by default) and per run (Run panel). For the editor: **Go to symbol** — Ctrl+T (or `#` in Ctrl+P)
    across the project, Ctrl+Shift+O (or `@`) in the current file; choosing one opens the file at its line.
  - Validated: Rust tests for every language's patterns, comments and control flow skipped, classes holding their
    methods, index refresh after edits and deletions, the MCP handshake, each tool's answer, paths outside the project
    and bad ranges refused, the session config; `npm run verify` green (61 frontend, 43 Rust tests). By hand: the real
    `qanuvi.exe --mcp E:\Projects\qanuvi` answered `find_symbol nextModel` with `src/lib/plan/runner.ts:70`, an outline,
    and a map (73 source files, 542 definitions). In the app: Ctrl+T "nextmo" found `nextModel` and opened it at its
    line; Ctrl+Shift+O listed the file's definitions.
  - Measured (Sonnet, lean sessions, the Qanuvi code, two questions, each asked twice in both orders): a question that
    needs finding and reading code across files cost on average $0.0605 without the tools and $0.0371 with them
    (**−39%**, fewer turns); a question one text search answers cost about the same ($0.0273 vs $0.0281, the tool
    descriptions cost a few hundred tokens); both together −26%. Answers were correct with exact lines.
  - Not done: references (who calls a function) — patterns find definitions only; a language server would add them
    (with the editor's own language support, later).
- [x] **12. Work packets**: the app prepares the step text, relevant files and lines, rules and test command for the
  model; the app runs tests itself and sends back only failures; measured against a plain agent run on the same step
  (tokens before and after). (done 2026-11-10)
  - Built: Rust `checks.rs` (runs the project's test command in its folder through the system shell, with `CI=1` and no
    colours, a time limit (default 10 minutes) that stops the whole process tree (`taskkill /T` on Windows, a process
    group elsewhere), stdout and stderr kept (the last 256 KB), never waiting more than 2 s for output a leftover
    program keeps open); `plan/packet.ts` (what a step mentions: file paths and code names; **where** that code is,
    from the index, one line per definition plus outlines of mentioned files and which files are new; the failure
    summary: failure lines with a little context and the end of the log, without passing lines, zero counters,
    colours or the test runner's own stack frames, at most 3,000 characters; the fix message); the run: "Work packets"
    option (Run panel, on by default) adds the locations and the rule "Qanuvi runs the test command when you finish;
    do not run the whole suite" to each step; after the agent's turn Qanuvi runs the tests, and on failure sends only
    the summary as a follow-up in the same session, at most 2 fix rounds; a step counts as done only when it is marked
    done **and** the tests pass; tries record how often the tests ran and whether they passed; the banner shows
    "running the tests".
  - Validated: Rust tests for the check runner (pass, fail with both streams, time limit stopping a child program in
    under a second, long output cut to its end); frontend tests for mentions, locations, the failure summary (noise
    removed), and the prompts; `npm run verify` green (69 frontend, 46 Rust tests).
  - Measured on a small Node project with a real test suite (`node --test`), three steps on Sonnet, lean sessions and
    index tools on, one run each: **A** no packets (the agent runs the tests) $0.1778; **B** packets with the code
    pasted in $0.1852 (+4%); **C** packets with locations only $0.1633 (−8%). All runs finished with 11/11 tests.
  - Found: pasting code does not pay off with Claude Code, because its Edit tool needs the file read in the session
    anyway, so the code is paid for twice; packets therefore give locations only. On a project this small the gain is
    within run-to-run variation; it should grow with bigger files (locations instead of searching) and big or
    failing test suites (a 1,000-character failure summary instead of the whole log).
  - Fix rounds checked for real: a step 4 plus a hidden failing test it did not mention; the agent marked it done,
    Qanuvi's full run caught the failure and sent ~1,000 characters (the failing test, `'12.5%' !== '12.5 %'`, file and
    line) instead of the log; after two rounds all 13 tests passed. The first round was lost because "work only on
    this step" conflicted with fixing a failure the step did not cause; the fix message now says to fix such a failure
    only when the fix is small and clearly right, otherwise mark the step blocked. Not measured again after that change.
  - Also seen: every step ends with a `Grep` for AI attribution words, from the owner's global agent instructions; it
    costs one tool call per step and is wanted.
- [x] **13. Learned routing and budgets**: success rate and cost per model and job type, used to choose models; rules
  per project; a monthly budget that prefers cheaper models or stops and asks. (done 2026-11-10)
  - Built: `plan/routing.ts` (job type of a step from its title and details: docs, tests, interface, money and
    security, bug fix, refactor, feature — money and security wins over the rest, plurals understood; per job type and
    model the tries, successes and cost, learned from the latest 300 runs of every project, tries the user stopped left
    out; the learned choice is the cheapest model by cost per success with at least 3 tries and 80% done; a step's
    first model: its own `- Model:` line, else the project's rule for its job type, else the learned model, else the
    default; monthly budget: from 80% used one model cheaper (never for money and security), at 100% the run stops and
    asks); Rust `usage_since` (all sessions' cost since the start of the month) and `runs::list_all`; the run records
    each step's job type and why it started on its model (shown in the step's chip); `RoutingSettings` in the Agents
    panel: monthly budget with this month's spending and a meter, rules per job type for the current project, and the
    "learned so far" table (tries, % done, cost per done step, "trusted").
  - Validated: routing tests (job types, plurals, learning, the learned choice, budget states, choice order, cheaper
    under a tight budget except for money and security), settings tests (budget and rules cleaned), Rust tests for the
    month's usage and listing runs; `npm run verify` green (79 frontend, 47 Rust tests). In the app: the learned table
    showed today's runs (for example features on Sonnet: 13 tries, 85% done, $0.07 per done step, trusted); with a
    monthly budget of $0.50 and $1.00 spent the run stopped at once ("needs you", $0 spent); with the budget cleared
    and the rule "docs → Haiku", a README step was recognised as docs, started on Haiku because of the rule and
    finished for $0.0035 with the tests passing.
  - Not checked live: the "one model cheaper" move under a tight budget (covered by tests), and a learned choice
    taking over after enough tries (covered by tests; today's data has no job type with a cheaper trusted model yet).
  - Not done: grouping steps by model to keep the agent's cache warm; job types from the files a step changes rather
    than its words.
- [x] **14. Planner and workers**: a paid planner turns a big request into a plan with a model per step; the controller
  runs it and goes back to the planner only on failure or for judgement. (done 2026-11-10)
  - Built: `plan/planner.ts` (the planner's request: read the project read-only and answer with 2–10 steps in one
    \`\`\`tasklist block, each with a Validate line and a model chosen from the agent's models; reading its answer — only
    step lines and their details, all opened; numbering after the file's last step, or 12a, 12b… for a replacement;
    adding a new section at the end of the task list with its line endings kept; replacing a failed step: it is marked
    `[!]` with a note naming its replacement, and the new steps follow its details; a section title from the request's
    first sentence); `stores/planner.svelte.ts` (the planner runs as an agent session in **plan mode** (read-only),
    lean, with the index tools, on the second strongest model by default; its final answer is taken from the result,
    the session closed; it shows live in the agent pane); `PlannerDialog` in the Plan view (wand button): the request,
    the planner model, "Make the plan", then a preview of the steps with their kind of work and model, a section title,
    and Discard / Add to the file / **Add and run** (runs the new steps with Auto, lean, index tools, packets and
    replanning). Qanuvi, not the planner, writes the file. In runs: "Back to the planner when a step fails on every
    model" (on by default): the planner gets the step, every try with its result and the last test failures, and
    rewrites it once into 2–5 smaller steps that are written into the file and added to the run.
  - Validated: planner tests (reading answers, renumbering, sections with CRLF, replacing a step, prompts, titles);
    `npm run verify` green (88 frontend, 47 Rust tests). For real on the test project: the request "add optional VAT
    to the cart total using addVat, with tests; document it in the README" was planned by Opus in 20 s for $0.14 into
    2 concrete steps (VAT with seven named test cases on Opus, the README on Haiku); "Add and run" wrote them as section
    steps 6 and 7 and ran them on those models ($0.152 and $0.005), Qanuvi's test runs passed, 20 of 20 tests pass.
    Whole request about $0.30.
  - Not checked live: replanning a step that failed on every model (covered by tests; forcing a real failure on the
    strongest model was not worth its cost); the planner's answer when it refuses or asks a question (the dialog shows
    "no steps" and the answer stays readable in the agent pane).
  - Found while testing: the dialog's buttons can be below the visible part of the Plan view when the preview is long
    (scroll to reach them); a sticky button row would be better.
- [x] **15. Test browser** (done 2026-11-10; short recordings left for later): a headless browser started only for checks;
  screenshots and short recordings shown in the agent pane; pointer in the preview to send an element to the agent.
  - Built: `browser.rs` (finds Chrome, Edge, Chromium or Brave, or `QANUVI_BROWSER`; starts it headless with a new empty
    profile in the temp folder; a small DevTools client over a local WebSocket; notes console errors, uncaught
    exceptions, failed requests and 4xx/5xx answers, answers alerts with OK; waits for load plus half a second of quiet
    network), `page.js` (runs in the page: text, links and fields with numbers, find by number/selector/label text,
    element at a point), `browser_mcp.rs` (10 tools: `browser_open`, `_read`, `_links`, `_fields`, `_click`, `_type`,
    `_wait_for`, `_errors`, `_screenshot`, `_eval`; started as `qanuvi --mcp-browser <project> <state file>`; a click or
    typing answers only what changed: new address, new text lines, new errors), `preview.rs` (the preview tab's
    inspector and the agent's last page). `mcp.rs` now serves any tool set. Sessions get one MCP config with the index
    and/or the browser (`mcp/<session>.json`, old ones removed after a week); setting "Give it Qanuvi's test browser" in
    Agents (on by default; plan runs follow it, the planner does not get it since it plans read-only). The agent pane
    shows screenshots under their step, and MCP tool names without their prefix. The preview tab got three buttons:
    follow the agent (shows the page its browser is on), send this page to the agent (as text, into the message box),
    and point at an element (a picture of the page from the inspector browser; click an element and its selector, text
    and HTML go into the message box; scroll moves the page; Esc stops).
  - Light: the browser runs only while a session uses it or the inspector is open (stopped after 3 idle minutes and
    when the app closes). Measured with the light switches (no GPU process, one page process, no component extensions,
    network service in the main process): 6 processes, 109 MB private memory (it was 11 processes and 278 MB without
    them); 0 processes once the session ends. On Windows a job object ends the browser with its owner even when that is
    killed (tested with a forced kill: 0 processes 2 s later). Opening a page takes under a second.
  - Token cost: the 10 tool definitions add 337 tokens at session start (8,880 → 9,217, Haiku, lean), since the agent
    loads MCP tool details only when it needs them; so the setting is on by default.
  - Validated: 56 Rust tests (two drive a real browser: open, fields, type by label, click with the change answered,
    errors, screenshot and state file, eval; the inspector shows, picks `#total > span` and reads a page) and 90
    frontend tests (screenshot items, tool names, settings). Live: Sonnet checked a test shop page with a hidden bug
    using only the browser tools: it filled in the coupon by number, clicked, got the `TypeError` at line 15 and the
    missing stylesheet, and said why the total stayed 50.00: 7 turns, 15.8 s, $0.024, no screenshot (the first version,
    with full file addresses and no "open a page first" answer, took 9 turns and $0.045). In the app: the inspector
    showed the owner's local site and a click on its logo put the element (an `img` with alt "Amirul Arham", its
    selector and HTML) into the message box; closing the app ended the inspector's browser.
  - Not done: short recordings; follow-the-agent was not tried with a live session in the app (covered by the state-file
    test); the browser's end with its owner on macOS and Linux relies on the normal shutdown only (Milestone 16 should
    add the Linux equivalent of the job object).
  - Agents use Qanuvi's browser instead of launching their own (added by the owner 2026-11-10): Qanuvi offers its
    browser to agents as tools (an MCP server Qanuvi starts with each session: open address, read the page as text,
    list links and form fields, click, type, console errors, failed requests, screenshot on request), so Playwright or
    a separate Chrome is not started for checks.
  - Token saving: answers are short text by default (the page's visible text and structure, errors only), and a
    screenshot is sent only when asked for or when text cannot answer the question.
  - The preview tab can show what the agent is looking at, and the user can hand the agent the page or an element
    from the preview.
  - Note: the preview tab today is a frame inside Qanuvi's own page, which cannot be read or driven for pages from
    another address; this step uses a browser controlled from the Rust core (Chrome DevTools Protocol) instead.
- [x] **15b. Git like VS Code** (added by the owner 2026-11-10; done 2026-11-10, comparison with VS Code on the owner's
  own repositories still to do by the owner)
  - Done earlier: `git/decorations.ts` (letters M, A, U, D, R and `!` for conflicts from `git status`; every folder above
    a change gets a dot) and the file tree showing them in VS Code's colours, refreshed after each agent turn, after each
    save and every 15 seconds while the window is shown (so terminal commands show up too).
  - Built: `git.rs` (status with upstream and ahead/behind; a file's last-commit and staged text in working-tree form
    with `cat-file --filters`, so line endings match the file; staged text set directly with `hash-object --path` and
    `update-index`, which is how one change is staged or unstaged; unstage, stage all, commit (message on standard
    input, amend, `--cleanup=strip`), fetch / pull / push (push sets the upstream the first time; git never waits for a
    password in a terminal, the user's credential helper still works), branches (list, switch, create, a remote branch
    gets a local one that follows it), stash (push with untracked files, pop, apply, delete), log of the project or a
    file (following renames), what a commit changed, blame of one line). `files.rs` lists ignored entries too, marked,
    and the tree greys them and everything under an ignored folder (`.git` stays hidden). `git/hunks.ts`: a line diff
    (Myers, as git; nearby changes stay apart), stage / unstage / undo one change, gutter marks, conflict blocks (also
    three-way ones) and their resolution. `git/gutter.ts`: change markers in the editor (green added, blue changed, red
    removed) against the staged text, recomputed 400 ms after typing stops, refreshed after saves, agent turns and git
    actions. Alt+B in the editor: who last changed the line, when and in which commit. The Diff tab became **Git**
    (`SourceControl.svelte`, `GitDiff.svelte`): branch and ahead/behind, pull, push, fetch, stash, project history;
    message box (Ctrl+Enter), Commit (or "Commit all", which asks to stage everything first), amend; Merge changes,
    Staged changes and Changes with per-file stage, unstage and undo (undo is confirmed); a file's diff side by side or
    inline (CodeMirror's merge view, unchanged parts folded) with the changes numbered and Stage change / Unstage change
    / Undo change (confirmed) for the current one; conflicts as blocks with Accept current / incoming / both, then Mark
    as resolved; a file's history. In a narrow pane the lists go above the diff.
  - Validated: 58 Rust tests (a real repository: part of a file staged and unstaged, commit with an empty message
    refused, history, a commit's change, blame, branches with a bad name refused, stash with an untracked file and back,
    push without a remote explained) and 102 frontend tests (line diff, staging and undoing one change also with CRLF and
    without a final line break, gutter marks, conflicts). In the app on a test repository with a local remote: the
    conflict was listed under Merge changes, the first of two changes in a file was staged alone (git showed `MM`, only
    that change staged), the conflict was resolved with Accept incoming and marked resolved, the commit was made with
    Ctrl+Enter, and Push sent it to the remote (0 ahead afterwards); the gutter showed removed, changed and added lines
    and kept them apart while typing; Alt+B showed the line's commit; `debug.log` was greyed by `.gitignore`.
  - Found and fixed while testing: the first gutter used CodeMirror's character diff, which merged nearby changes after
    an edit (replaced by the line diff); the file tree kept an old letter after a commit (now refreshed after every git
    action); push said only "Done." (now says where it pushed); buttons were cut off in a narrow pane (they wrap, and the
    view stacks).
  - Not done: a VS Code side-by-side check on the owner's own repositories; staging from the gutter itself (it is done
    from the Git view).
  - File tree decorations as in VS Code: modified `M`, added `A`, untracked `U`, deleted `D`, conflict `!` with their
    colours; a dot on folders that contain changes; ignored files and folders shown greyed.
  - Diff in the work pane: side-by-side and inline views, change markers in the editor gutter (added, changed,
    removed), stage and unstage per file and per change, revert a change (confirmed first).
  - A Source Control view: changed and staged files, commit message box, commit, push, pull, fetch, branches (switch,
    create), stash, history of a file, blame on a line; merge conflicts with accept current / incoming / both.
  - Git from the terminal keeps working as normal (the terminal is a real shell, Milestone 5); the tree, gutter and
    Source Control view refresh when files or the repository change, whether from the terminal, an agent or the editor.
  - Git runs through the installed `git` program, so the user's own configuration, credentials and hooks apply.
  - Validate: the owner's repositories (for example a Qlopy site with modified and untracked files) show the same
    status as VS Code; commit, push and pull on a test repository; a conflict resolved from the view.

- [x] **15c. Workspace per project and a second window** (added by the owner 2026-11-10; done 2026-11-10, layout and
  scroll position per project left out)
  - One Qanuvi window stays the normal way of working (one light app process, projects switched in the sidebar), and
    each project remembers its whole workspace: open files with cursor and scroll position, the layout, the shown view,
    terminals, the preview address and the agent session it was on; switching back restores all of it, also after a
    restart.
  - Optional **Open in a new window** for a project (from the project's menu in the sidebar, and a command), for working
    on two projects side by side, for example on two monitors. The second window shares the app process and its data;
    a project open in one window is shown as such in the other, so the same files are not edited in two places.
  - Validate: switch between two projects and back, and restart: tabs, cursors, layout and terminals come back; two
    windows on two projects work side by side; closing one leaves the other running.
  - Built: `stores/workspace.svelte.ts` (per project in local storage: open files with line and column, the active
    file, the shown view, terminals by shell, the agent session, the preview address; saved half a second after a
    change and when the window closes; read back with checks: no paths with `..`, only project views, only http(s) or
    file preview addresses). On a project's first opening in a run its files come back with their cursors (files that
    are gone are dropped), then its view and terminals; on every opening, the agent session it was on. The editor
    keeps each tab's cursor and can open a file at a line and column. `stores/windows.svelte.ts`: **Open in a new
    window** (a button on each project in the sidebar) opens a window of the same app process for that project, or
    brings forward the window that already shows it; windows tell each other which project they show (a new window
    asks), the sidebar marks a project open in another window, and choosing it goes to that window instead of opening
    it twice. A project window does not change which project the main window starts with, and joins the server
    connections the app already has. Rust: terminals remember the window that opened them; closing a second window ends
    only its terminals (the last window ends everything as before) and tells the other windows. The window capability
    covers project windows.
  - Validated: 108 frontend tests (saved workspaces read back safely, routing). In the app: after a restart `cart.txt`
    came back at line 5, column 7; a VPS project opened in a new window with its file and its server terminal back;
    the main window's project was marked in the new window; closing the project window left the app running, closing
    the main window ended it and its SSH connection.
  - Found and fixed while testing: a new window did not see the server projects (it now joins the app's open
    connections) and fell back to the main window's project (a project window now waits for its own); a closed window
    still showed as open elsewhere (the app now announces it); the marker hid the project name (now an icon).
  - Also checked in the app: choosing in a project window a project the main window shows brought the main window
    forward instead of opening it twice; closing the project window removed its mark from the main window's sidebar.

- [ ] **15e2. Terminal list like VS Code (cosmetic, owner 2026-11-11, later)**: instead of the shell drop-down and tabs
  on top of the terminal panel, a slim list of terminals on the right side of the panel (icon + name, warning mark,
  current one highlighted), with + and the panel buttons above it.
- [x] **15f. Notices in the status bar (owner, 2026-11-11; built 2026-11-11)**: when a sign-in needs renewing (OmniRoute
  connections such as Antigravity or Kiro, OpenRouter key problems), a notice in the bottom bar that opens the renew
  screen.
  - Built: a watch at start and every 30 minutes (when Qanuvi runs OmniRoute) reads OmniRoute's connections; a sign-in
    to renew or a connection that stopped working shows in the status bar ("kiro: sign in again (+1)", opens the
    Agents page with the OmniRoute panel) and in the Inbox; it leaves both by itself once solved. Unnamed connections
    go by their provider. Live: kiro and kimi-coding showed. Not yet: OpenRouter key problems (the key check runs
    only from the panel today).
- [x] **Inbox (24f, 15f; built 2026-11-11)**: the Inbox page (sidebar → Inbox): sessions waiting for approval, then
  what happened — plan run reports (finished / needs you / stopped, with the summary of done, skipped and blocked
  steps and "Open the plan"), sign-ins to renew, newer knowledge from Qanuvi. Kept on this computer (last 100); the
  sidebar's number counts waiting sessions and unread items. Live: a one-step run's report and two sign-ins listed.
- [x] **15g. Task Manager shows an old icon (owner, 2026-11-11; checked by the owner 2026-11-11: the installed build shows the current logo)**: the program and its window carry the
  current logo (checked by colour); Windows' icon cache for the program's path still shows the first logo. An installed
  build (new path) should be right; check after the release, and whether `ie4uinit -ClearIconCache` or a sign-out
  clears it.
- [x] **15h. Qanuvi's own layout, with presets (owner, 2026-11-11; built 2026-11-11)**: settings must not sit among the code tabs; the
  work area holds project work only. Qanuvi keeps a look of its own (a brand), clean like VS Code but not a copy, and a
  one-click preset gives VS Code users the layout they know. Few pages (owner): one settings page with tabs, the Inbox
  as a drop-down, Plan as a work-area tab.
  - Default layout ("Qanuvi"): the labelled sidebar (Home, Inbox, Projects, Plans; projects and sessions; Settings at
    its foot), the conversation in the centre, the work area on the right (tabs: files, Preview, Plan), terminal and
    status bar at the bottom.
  - [x] Step 1, the settings page (2026-11-11): one page over the main area (the panes stay underneath, terminals keep
    running), tabs on the left: Agents, Models and providers, Team, Servers, Memory, Account; a search box that narrows
    the tabs by what they hold (Enter opens the first); x or Esc closes it. Opened by Settings at the sidebar's foot,
    and by the sidebar's Agents, Memory and Servers and every former link to those pages (status bar, Inbox, agent
    pane), at the right tab. Opening a configuration file from it closes it and shows the file as an editor tab. Live:
    every tab, search ("omniroute" -> Models and providers), Esc, the config file and the sidebar entries checked.
  - [x] Step 2 (built 2026-11-11): the work area's tabs are files, Preview and Plan; the project panel on its left edge (owner: like VS
    Code's side panel, where nothing is there to be ignored in a coding app), opened and closed with one button,
    resizable, with an icon row switching Explorer, Search and Git. The Explorer has collapsible sections that can be
    resized, reordered by dragging and hidden: Open editors; Files (tree with new file / new folder / refresh /
    collapse all); Outline (symbols of the current file, from the code index); Timeline (the file's commits and
    saves, plus Qanuvi's entries for changes an agent made: which session and model, opening that session); as
    an option, "Touched in this session". The Inbox becomes a drop-down from the sidebar.
    Built: the panel (Explorer / Search / Git icons; a "..." menu shows or hides sections); sections open and close,
    drag into another order, resize from their lower edge; Files has New file / New folder (files_create: never
    replaces, stays inside the project) / Refresh / Collapse; Outline from the code index; Timeline: agent changes
    (agent_file_history: sessions whose tool calls changed the file, any path form, reads left out; opens the
    session) and commits (open the history in the work area); "Touched in this session" (hidden at first). The Git
    view is split: its list in the panel, the diff and history in the work area under a "Changes" tab. Search moved
    into the panel (Ctrl+Shift+F opens it). The Inbox is a drop-down beside its sidebar entry (Esc or a click outside
    closes it). Live: Outline of agent.rs, Timeline of notes.txt (5 agent sessions) and agent.rs (3 commits), New
    file, a change opening its diff, Search, the Inbox.
    Changed after the owner tried it (2026-11-11): four columns were too many (sidebar, conversation, panel,
    editor) and Files was squeezed. The panel now lives in the left sidebar behind a mode switch at its top
    (Workspace | Explorer | Search | Git; collapsed, the icons stand in a column and open the sidebar); the editor
    gets the room back. Only Files starts open; other sections are as tall as what they hold (up to a height
    dragged from their lower edge), so an empty one takes no room. The tree button in the work area switches the
    sidebar to Explorer and back; Ctrl+Shift+F shows Search there.
  - [x] Step 3 (built 2026-11-11): presets in the layout button (one click): "Qanuvi" (default), "Coding" (code larger, conversation
    narrower), "VS Code-like" (icon bar on the far left, Explorer/Search/Git in a left side panel, editor centre,
    conversation on the right like a chat side bar, terminal bottom); plus Appearance and Layout tabs in Settings.
    Built: layouts also hold the conversation's side and the sidebar's mode; presets Qanuvi (default), Coding,
    VS Code-like, Side screen, Review, each with a line saying what it is for; Settings → Layout (pick one, the
    conversation left or right, what the sidebar shows, save under a name, remove own layouts) and Settings →
    Appearance (editor font size 10-24 px; interface size 80-150 %, the window's own zoom). The sections menu (...)
    sits in the sidebar's switch row. Live: VS Code-like (files left, editor middle, conversation right), font 17 px
    and 115 %, back to Qanuvi and the usual sizes.
- [ ] **15i. A language-aware editor and debugging (owner, 2026-11-11; after the self-development build runs
  reliably)**: the editor becomes as strong as VS Code's for real work, through the same open standards.
  - Language servers (LSP): completion, errors and warnings underlined plus a Problems list, hover, exact go to
    definition and find references, rename a symbol, formatting, quick fixes. PHP first (Intelephense or Phpactor),
    then TypeScript / JavaScript / Svelte, Rust (rust-analyzer), Python (Pyright), HTML / CSS / JSON.
  - Debugging (DAP): breakpoints in the gutter, continue / step, variables, call stack and watch in a "Run and debug"
    sidebar mode, a debug console; VS Code's launch.json read as it is. PHP with Xdebug and Node first, then Python
    and Rust (CodeLLDB).
  - Qanuvi's own part: agents use the same language intelligence (they see the project's errors; "Fix with agent"
    on any problem; renames and reference searches through the server: more exact, fewer tokens).
  - Light, unlike VS Code (owner asked): nothing bundled (each server or adapter installs on demand, with the
    user's yes); a server starts on the first file of its language and stops when the project closes or after a
    while unused; debuggers run only while debugging; Qanuvi checks free memory before starting a big one (as for
    local models) and says so when short; each language can be turned off in Settings.
  - Order: 1. LSP for PHP, TypeScript / Svelte, Rust, with the Problems list. 2. Formatting, quick fixes, "Fix with
    agent". 3. Debugging (PHP / Xdebug and Node first). 4. Split editor, minimap.
  - Built (part 3, session A, 2026-11-14; not driven live): src-tauri/src/dap.rs installs js-debug (latest GitHub
    release of microsoft/vscode-js-debug, js-debug-dap-*.tar.gz, unpacked with the system tar into the data folder's
    debug-adapters/node; the old copy is kept until the new one is whole) and runs it as a local-port server, one
    connection per child session (its startDebugging request). Window: src/lib/dap (launch.json with comments and
    ${...} variables, DAP client, breakpoint gutter that follows edits, paused line), src/lib/stores/dap.svelte.ts (one
    session at a time; breakpoints, watch and picked configuration kept on this computer), "Run and debug" sidebar mode
    (DebugPanel), "Debug console" bottom tab (DebugConsole). Keys as VS Code: F5, Shift+F5, Ctrl+Shift+F5, F6, F9,
    F10, F11, Shift+F11. Without launch.json: "run the current file" and "attach (port 9229)"; the gear writes VS
    Code's form. Not built: ${input:...} / ${command:...} (says so), runInTerminal, exception breakpoints UI, hover
    values, logpoints UI, agent access to the debugger.
  - Driven live (part 3, session A, 2026-11-14, owner's yes for the js-debug download; release test build on a
    scratch Node project): WORKS. The install offer showed on F5, Install took seconds and debugging started by
    itself; F9 and gutter clicks add breakpoints (list in the sidebar); paused at app.js:13, Continue stopped at
    line 4 with the right scopes and the paused arrow; watch (it.price * it.qty: 6, then 10), F10 step over, console
    evaluate in the selected frame (sum * 10 = 60), run to the end ("total is 16", "Debugging ended."), no node
    processes left; the gear writes a formatted VS Code launch.json and the dropdown switches to it. Peak about 650
    MB. Findings for the owner (not fixed yet): (a) the breakpoint gutter is a narrow 14 px strip left of the line
    numbers, easy to miss (a click on the number itself does nothing); (b) "Paused: breakpoint" and "Paused on
    breakpoint" say the same thing twice; (c) the configuration dropdown cuts names ("Node: run the currer");
    (d) a watch shows "not pa…" when not debugging; (e) once, launch.json read back from disk as one line right
    after the gear and was formatted 30 s later; a repeat was formatted from the first write, not reproduced.
    Not a bug: a stray click on an internal call-stack frame shows that frame's scopes without an arrow (correct).
  - Fixed (owner agreed, 2026-11-14, 4abc7de, not driven live): (a) a click on the line number also toggles a
    breakpoint, and hovering a number shows the faint dot in the strip; (b) one label, the adapter's description
    ("Paused on breakpoint") or "Paused on <reason>", and the extra note only when its text differs (exception
    text); (c) the dropdown shows the full name on hover and ends cut names with an ellipsis (the sidebar width
    still limits it; the open list shows full names); (d) a watch shows "–" with "Not debugging" on hover; (e) the
    gear writes launch.json, reads it back and writes again (up to 3 times) before it opens the file.
  - Session B notes (2026-11-14): Xdebug 3.5.3 DLL in E:/DevTools/xdebug, loaded per run only (php.ini untouched:
    Wnmp's PHP serves live sites). php.ini still holds Xdebug 2 keys (xdebug.remote_*), so Xdebug 3 prints
    "[Config] ... has been renamed" warnings on start: Qanuvi passes the Xdebug 3 settings itself (xdebug.mode=debug,
    xdebug.start_with_request=yes, xdebug.client_port=9003) and should keep those warnings out of the console.
  - Built (part 3, session B, 2026-11-14, bc0b0ae): dap.rs knows a second adapter, "php" (xdebug/vscode-php-debug,
    the .vsix of its latest GitHub release, about 2 MB, unpacked with the zip crate because Linux's tar cannot read
    zip; entry extension/out/phpDebug.js over stdio). dap_php finds PHP (version, NTS/TS, x64) and its Xdebug: php.ini's
    own, else the "Xdebug file" from Settings > Languages, tried with -d zend_extension and loaded for debug runs only
    (php.ini untouched). A PHP run gets the found php, the file and xdebug.mode=debug, start_with_request=yes,
    client_host=127.0.0.1, client_port=${port} (port 0: PHP Debug picks one), unless launch.json's runtimeArgs set
    them. Defaults: "PHP: run the current file", "PHP: listen for Xdebug (port 9003)"; the gear writes VS Code's PHP
    launch.json for a PHP file. The "has been renamed" warnings from php.ini's Xdebug 2 keys are taken out of the
    console and named once in one note. Settings > Languages has a Debugging part: each debugger (Install, version)
    and PHP / Xdebug with the file field and Check.
  - Driven live (part 3, session B, 2026-11-14, release test build, scratch PHP project): WORKS. F5 offered PHP Debug,
    Install took seconds (v1.40.2); without the Xdebug file the run stopped with the "no Xdebug, choose it in
    Settings > Languages" text; with the file, Settings showed "PHP 8.2.3 NTS x64 · Xdebug 3.5.3 (from the file
    below)". Paused at cart.php:15 (Locals), Continue stopped at line 6 inside total() (call stack total / {main}),
    watch $sum 0 then 6 after F10, console $sum * 10 = 60, run to the end "total is 16", exit code 0, no php or
    adapter left. Listen mode: PHP started from a shell with Xdebug stopped at line 15, Shift+F5 freed port 9003. The
    (a)-(e) fixes all work: number click toggles, hover dot, one pause label ("Paused on breakpoint" / "Paused on
    step"), full name on hover, "–" watch, gear's launch.json formatted at once. Peak about 630 MB (with
    Intelephense). Two findings fixed and driven live: the renamed-settings note named only the first piece of
    PHP's warnings (3 of 5 keys; now all, 400 ms after the first); the sidebar kept "no Xdebug" after Settings fixed
    it (now cleared by the check). The install offer said the debugger's name twice; owner agreed to shorten it:
    now "<name> is needed. Downloads its latest release from GitHub (…)" (not driven live).
  - Built (part 4, 2026-11-14): split editor and minimap.
    Split: at most two editor groups, side by side or one above the other (button at the right of the work area's tab
    row flips it; the split and its size are kept in localStorage qanuvi.editorSplit). Each group has its own tabs and
    active file; a file can be open in both (src/lib/editor/groups.ts: the rules, with tests). Both views of one file
    stay in step (src/lib/editor/shared.ts: a change typed in one is copied into the other, not added to its undo
    history; a state handed between groups is reconfigured to the receiving editor, keeping text, selection, undo and
    language). Ctrl+\ opens the file in the other group (splitting when there is one), Ctrl+1 / Ctrl+2 move the
    keyboard, Ctrl+W closes the tab in the focused group. Tabs drag (pointer events, src/lib/stores/tabDrag.svelte.ts)
    to another place, to the other group, or onto the right side of the editor to split it; the tab menu has Open to
    the side / Open below / Open in the other group / Move to the other group; the × on a group's strip joins the
    groups. Opening a file goes to the focused group (as VS Code). A reload (disk version) now raises tab.version so
    every editor showing the file starts again. The language-server actions find the file's live view in either
    group. Each project's workspace keeps which group shows which file (SavedTab.groups, active2, focused).
    Minimap (Settings > Appearance, off by default): src/lib/editor/minimap.ts paints one canvas, 2 px per line, only
    the lines that fit, in the syntax colours (theme.ts minimapHighlighter); selection, search hits while the search
    panel is open, git changes on the left edge, the whole file's errors / warnings / search hits on the right edge;
    click a line to centre it, drag the slider, wheel scrolls.
  - Driven live (part 4, 2026-11-14, release test build, scratch git project with a 300-line long.js and notes.md):
    WORKS. Ctrl+\ split long.js into two groups; typing in the right group showed at once in the left (same git mark
    and unsaved dot in both); two Ctrl+Z in the right undid it in both and both went back to saved; the layout button
    put the groups one above the other and back; notes.md opened in the focused group; dragging its tab into the other
    group's strip moved it; the minimap switch showed the minimap in both groups with syntax colours, the slider and the
    git mark; a click on the minimap centred the clicked line (221); dragging the slider scrolled; Ctrl+F "step4"
    marked the hits on the right edge; after a restart both groups, their files and the minimap came back; the
    group's × joined the groups; dragging a tab onto the right side of the editor split it again. The new logo shows in
    the title bar; the built qanuvi.exe icon has the new colours (checked by pixel; the centre square is transparent
    in the owner's file, so the taskbar shows through it). Not driven: Ctrl+1 / Ctrl+2, Open below from the menu,
    errors on the minimap (no JS server installed in the test data), reorder within one strip.
    Findings for the owner (not changed): (a) with one file in both groups, the language-server install offer shows
    in both; proposed: show it in the focused group only. (b) a tab given back after a restart (or shown from a kept
    state) has its cursor line in the status (Ln 83) but the view stays at the top (as before part 4); proposed: scroll
    the cursor into view when a kept state is shown.
    BOTH FIXED (owner: "do as you suggested", verify 375 + 172) and driven live in a new release test build: with
    long.js in both groups only the focused group shows the install offer; a file opened in the other group (Ctrl+\)
    shows its cursor line in the middle; switching tabs in a group and back returns to the same scroll place (line 203
    stayed on top). How: each editor keeps a scroll snapshot per file when it leaves it (dropped when the text length
    changed since), else scrolls the cursor into view for a kept state.
  - LATER (owner, 2026-11-14): Phpactor as a second PHP server to choose in Settings > Languages. It is free and
    open source and gives quick fixes and rename, which free Intelephense keeps for its paid licence (VS Code users
    on free Intelephense have the same gap). Phpactor needs PHP 8.1+ on the machine (it is a PHP program, not npm),
    so the install offer must check `php` first and say so. Choice per project or global; one PHP server at a time.
  - Built (part 1, 2026-11-13; live check open): src-tauri/src/lsp.rs starts a server per project and language
    (Intelephense, typescript-language-server, svelte-language-server, rust-analyzer, Pyright), frames the messages,
    below-normal priority, a job object, the free-memory check; stopped with its window, its project, or the last
    window. Install on the user's click: npm servers go into the data folder's language-servers/<id> (nothing
    global), rust-analyzer through rustup; a server already on PATH is used as it is. Window side: src/lib/lsp
    (protocol helpers with tests, the JSON-RPC client, CodeMirror hover / completion / underlines, definition,
    references, rename across files incl. unopened ones), stores/lsp.svelte.ts (start on first file, full-text sync
    after a 250 ms pause, idle stop after Settings' minutes, problems per file), an install offer bar in the editor
    (Install / Not now / Don't ask again), a Problems tab (count, filter, by file, errors first) and a References tab
    in the bottom panel, F12 / Shift+F12 / F2 and the right-click menu (the code index stays the fallback for F12),
    Settings, Languages (status, Install, on/off per language, idle minutes). Projects on a server get none yet.
  - Live check (2026-11-13, release build, scratch project with PHP and Rust): Intelephense and rust-analyzer start
    on the first file, underlines and gutter dots, Problems tab (by file, codes, positions, "ready" per server),
    hover, completion, F12, Shift+F12 (References tab), F2 on Rust (3 places, file left unsaved), idle stop (rust-
    analyzer ended 10 minutes after its last use while Intelephense stayed for an open file), both servers end with
    the window. Memory: Intelephense about 113 MB, rust-analyzer about 430 MB once it has indexed std. Test builds
    keep data in dev.qanuvi.desktop.source, so a server installed by the installed app is not there (by design).
    Fixed from the check: (1) completion now asks for snippets and expands them with CodeMirror (protocol.ts
    snippetTemplate): a method gives total() instead of total, which Intelephense then flagged as an undefined
    property; (2) a server installed from outside the window (another window, npm or rustup in a terminal) is seen
    without a restart (statuses read again, at most every 30 s); (3) F2 asks the server first: no name prompt when
    it cannot rename (free Intelephense: "renames only with its paid licence"); (4) the name prompt gives the focus
    back (after F2, Ctrl+S did not save); (5) the status bar's Ln/Col follows a tab switch.
  - Built (part 2, 2026-11-13; not yet driven in the window): formatting: Shift+Alt+F the file, Ctrl+K Ctrl+F the
    selection (also in the right-click menu), one undoable change, the file's indent guessed and sent (tabs, 4 or 2
    spaces); nothing changes if the text changed meanwhile. Format on save (Settings, Languages; off by default):
    only when the file's server already runs and can format, at most 3 s, then saved anyway. Quick fixes: Ctrl+. (or
    the right-click menu, or "Quick fix" in a problem's hover box) opens a menu at the cursor with the server's code
    actions for the problems there (preferred fix first, quick fixes, refactors, then whole-file ones; disabled ones
    with the reason), resolved when the server sends them without the edit, then their command run; edits a server
    sends back (workspace/applyEdit) go through the same code as a rename (lsp/actions.ts applyWorkspaceEdit: the
    shown file one undoable change, other tabs left unsaved, closed files written). "Fix with agent" on a problem
    (hover box, Ctrl+. menu, right-click, the Problems list's row and its file's "Fix all with agent", right-click
    there with copy too) adds a request to the message box (place, message, server code, the lines around one
    problem); the user reads and sends it, so it costs nothing until sent. Pure helpers with tests: lsp/fix.ts.
    Checked against the real servers with a probe script on a scratch folder (same capabilities as the client):
    Intelephense (free) formats the file and the selection but has no code actions (the menu says they need its paid
    licence, "Fix with agent" is still there); rust-analyzer formats the file (no selection formatting: said so),
    gives quick fixes such as "Add missing match arm" and "Remove all unused imports" that resolve to edits. Open:
    drive it in the running app on a scratch project (menu at the cursor, hover box buttons, format on save, a
    rust-analyzer fix applied); agents seeing the project's problems by themselves is part of the later Qanuvi part.
  - Driven live (2026-11-14, test build of 07:59, scratch project with cart.php and a Cargo project, $0, no agent
    turn sent): WORKS: Ctrl+K Ctrl+F formatted only the selected line ("Formatted." note, tab unsaved), one Ctrl+Z
    undid it (tab clean again); Shift+Alt+F formatted the whole PHP file (indent guessed as 2); format on save (off by
    default, ticked in Settings > Languages) formatted cart.php with Intelephense and main.rs with rust-analyzer and
    saved the edit too; free Intelephense: Ctrl+. menu says "Intelephense offers quick fixes only with its paid
    licence" plus Fix with agent; Fix with agent (menu, hover box, Problems row, file header "Fix all with agent")
    only fills the message box: place, message, server code, lines around one problem; nothing sent. rust-analyzer:
    "Add missing match arm `Size::Large`" first in the menu, applied (todo!() arm), stale rustc error cleared on save
    (same as VS Code); "remove the whole `use` item" applied cleanly; right-click menu has Quick fix, Fix these N
    with agent, Format document / selection; Problems row menu: Open, Fix with agent, Copy, Copy with its place.
    Both servers ended with the window. FINDINGS for the owner, not changed yet:
    (a) BUG: Ctrl+. (or right-click Quick fix) with a SELECTION shows nothing at all. Cause: ContextMenu.svelte keys
        items by label (`{#each group as item (item.label)}`); rust-analyzer then sends two fixes with the same title
        ("remove the whole `use` item", one per import), Svelte throws on the duplicate key and no menu shows.
        Proposed: key by position, and tell same-titled fixes apart by line ("... (line 2)").
    (b) BUG: "Remove all unused imports" left all underlines across the whole file. rust-analyzer sends that fix as
        a rewrite of most of the file (line 1 changed, 307 characters inserted at its end, lines 2-23 deleted); the
        text comes out right, but the lint mark at the end of line 1 grows over the inserted text, and other marks
        are lost (also after undo). Proposed: in changesFor, when the server's edits rewrite far more than the real
        difference, apply the trimmed difference instead (common start and end cut off); renames keep their edits.
    (c) Ctrl+. menu opens with nothing highlighted, so Ctrl+. then Enter does nothing (ContextMenu `focused = -1`).
        Proposed: a menu opened from the keyboard highlights its first enabled item (VS Code does).
    (d) The hover box stays open under the Ctrl+. menu (also from the hover's own Quick fix button) and covers the
        code. Proposed: close the hover when the quick fix menu opens.
    (e) rustc's help hints ("remove the whole `use` item", severity hint) show in the hover as problems with their
        own buttons, and the right-click menu counts them ("Fix these 4 problems" where Problems says 2). Proposed:
        hints stay out of the problem list in hover and counts (Fix all with agent already leaves them out).
    Note: "Remove all unused imports" with the cursor on one import removes only that one (rust-analyzer works on
    the range sent, VS Code sends the same); with both lines selected it offers to remove both (seen in a probe).
  - ALL FIVE FIXED (owner: "continue", 2026-11-14; verify green 345 + 166) and driven live in a new test build:
    (a) menu items keyed by place (ContextMenu), same titles told apart by line (fix.ts actionLabels): the selection
    menu shows "remove the whole `use` item (line 1)" / "(line 2)"; (b) protocol.ts tightChanges: when a server's
    edits rewrite far more than the real difference, the trimmed difference goes instead (renames keep their edits);
    "Remove all unused imports" removed both imports, no stray underline, other marks moved with the text; (c) a menu
    opened from a key marks its first enabled item (contextMenu `fromKeys`): Ctrl+. then Enter added the match arm;
    (d) closeHoverTooltips when the quick fix menu opens: the hover closed; (e) hints get no buttons in the hover and
    are left out of "Fix these N problems" (actions.ts realProblems): the menu said 2. Known, not changed (VS Code
    alike): until the file is saved, problems keep the server's old lines, so a menu after a fix that moved lines
    may miss "Fix with agent" for a stale problem; saving refreshes them.
- [x] **15d. Own right-click menus** (added by the owner 2026-11-10; built 2026-11-11)
  - Today a right-click shows the WebView's browser menu (Back, Refresh, Save as, Print, More tools), which does not
    belong in an app, and its Refresh reloads the whole interface. Replace it with Qanuvi's own menus, by place:
    editor (cut, copy, paste, go to definition, find references, format, add to agent message, git: blame line, stage
    change), file tree (new file or folder, rename, delete, copy path, reveal in Explorer/Finder, open in terminal,
    git stage / discard), tabs (close, close others, close to the right, copy path, reveal), agent messages (copy,
    quote in reply, open file), terminal (copy, paste, clear), Git view (stage, unstage, discard, open file, history),
    sidebar projects (open in new window, settings, remove). Text fields keep cut / copy / paste / select all.
  - The browser's own menu and its reload shortcut are turned off in release builds (kept in development for
    inspecting the interface).
  - Validate: a right-click in each place shows only its own actions; nothing reloads the interface.
  - Built: one menu for the whole window (arrow keys, Enter, Esc, a click elsewhere) and a name prompt. File tree:
    open, add to the message, new file / folder (in that folder), open in terminal (in that folder), rename (open
    tabs follow), delete (to the Recycle Bin, after asking; its tabs close), copy path / relative path, reveal in File
    Explorer, git stage / discard; the tree's empty space: new file / folder, terminal, refresh, collapse, reveal.
    Tabs: close, close others, close to the right, close saved, copy paths, reveal, add to the message. Editor: go to
    definition (code index), cut / copy / paste / select all, add the selection or line to the message, who changed
    this line. Conversation: copy, quote in reply; tool steps: open the file, copy its path, copy the result.
    Terminal: copy, paste, select all, clear. Git view: open changes / file, stage or unstage, discard, history, copy
    path. Sidebar: projects (open, new window, settings, copy path, reveal, remove), sessions (open, rename, delete).
    Text fields: cut / copy / paste / select all. Elsewhere nothing in a release build, and F5 / Ctrl+R do not reload.
    Backend: files_rename (never replaces), files_trash (Recycle Bin), terminals start in a chosen folder. Live: every
    menu read through the release build; new file, rename, open in terminal and delete done for real.

- [x] **15e. Where Qanuvi keeps its data** (added by the owner 2026-11-10; built 2026-11-11)
  - Everything Qanuvi stores can live in a folder the user chooses, on any drive, not only the system drive (C: on
    Windows): the database (projects, sessions, memory), the brain's records (`brain.json`), the agent's conversation
    histories (`agent-history/`), the memory disk's working files, test-browser profiles, MCP configs, logs and
    caches. Chosen at first start or in Settings, kept in a small pointer file in the default place.
  - Moving it: copy to the new folder, check, switch, then offer to remove the old copy (confirmed). A server (master)
    already takes `--data`.
  - Validate: a fresh start with the data on another drive (for example E:) writes nothing to the old place except the
    pointer; moving an existing setup keeps projects, sessions, memory and the brain's records.
  - Built (data_location.rs, DataFolderPanel on the Servers page): choose a folder (or type it); a folder with other
    things gets a "Qanuvi" folder inside; checks: not inside the current folder, empty, writable, room on the drive.
    The move is booked and done at the next start, before the database and the window open: Qanuvi restarts itself
    and the new instance waits for the old one to end (--after-pid); everything is copied (the window engine's caches
    left out, files that vanish meanwhile ignored, retried while its helper processes finish), checked file by file,
    then switched; on any failure nothing changes and the reason is shown (red). The window's own storage (settings,
    guides, layout) moves too (WEBVIEW2_USER_DATA_FOLDER, Windows). Moving back into the default folder first clears
    the outdated copy there (so SQLite's old side files never sit next to a newer database). The old copy is kept until
    the user removes it (confirmed). Live on the owner's PC: moved to E:\QanuviDataTest — same 7 projects, 46 sessions,
    3 memory entries, the window reopened as it was, nothing written to C: afterwards (10 files changed on E:); moved
    back the same way. Two races found and fixed on the way (the old window still writing during the copy).
  - Not moved: Ollama's models (~/.ollama) and OmniRoute's data (~/.omniroute) belong to those programs; a later option
    can point Ollama at a folder on the same drive (OLLAMA_MODELS).

- [x] **16. Headless master**: master mode running without a window on a Linux server or VPS (16 GB), controlled from
  the desktop app. (Built 2026-11-10; tested on this PC and on the owner's VPS (Debian 12) over SSH.)
  - Built: the core builds without Tauri (feature `desktop`; `sink.rs` replaces the window's channels for agent and
    terminal events, `paths.rs` gives the data folder, the database handle is shared, the window's commands moved to
    `desktop.rs`). `qanuvi-master` (4.2 MB): `serve` (background service on 127.0.0.1 behind a token in a file only its
    user can read; agent sessions keep running without a desktop; `stop` also ends its sessions and terminals),
    `--stdio` (what the desktop starts over SSH: starts the service when needed and passes messages through), `status`,
    and the agent tools (`--mcp`, `--mcp-browser`). `master.rs` answers the window's own command names and arguments
    (projects, files, search, code index, checks, terminals, git, agent sessions, runs). Desktop: `remote.rs` (SSH with
    keys only, `BatchMode`, keep-alive; addresses and program paths checked so nothing can be slipped into the SSH
    command; plain-words errors for refused keys, unknown hosts, a missing program, an unknown host key; "local" starts
    the master next to the app for trying it without a server). Window: `remote.ts` routes each call to the master that
    owns its project, agent session or terminal; a **Servers** view (add, connect, disconnect, remove, connect at
    start-up, add a server folder as a project, setup steps including a systemd user service); server projects in the
    sidebar with a server mark.
  - Validated: 63 Rust tests (the master answers the window's commands; a connection answers each message, streams and
    closes its terminals; the service refuses a wrong token and answers with the right one; SSH address and program
    checks; error wording) and 106 frontend tests (routing). In the app with a master on this PC: connected (host,
    system, version shown), added a folder as a project, the tree with its git letters, the editor, a terminal (`git
    log` ran on the master) and an agent session (it used the code index served by the master program; 15 s, $0.08)
    all worked through the master; after closing the app the service kept running, and on reopening Qanuvi connected
    by itself, reopened the project and listed the session. Memory: service 8–24 MB, the SSH relay 8 MB.
  - Found and fixed while testing: the gutter marked every line when the file had LF line endings and git gave the
    staged text with CRLF (line endings are now compared as equal, as in VS Code); Tauri took the master as the main
    program (`default-run`).
  - Linux: built on Windows with `cargo zigbuild --release --no-default-features --bin qanuvi-master --target
    x86_64-unknown-linux-gnu.2.36` (zig 0.13 in `E:\DevTools\zig`, installed with the owner's permission): 4.75 MB,
    runs on Debian 12's own libraries. On the owner's VPS (Debian 12, 6 cores, 5.7 GB) over real SSH: hello in 557 ms
    including starting the service, then 50–90 ms per call (files, git status and diff, code index, search); a bash
    terminal on the server; the service file private (`-rw-------`), listening on 127.0.0.1 only, 20 MB of memory. From
    the app: a server with a key file (new optional "SSH key file" per server, `-i` with `IdentitiesOnly`; `~` allowed)
    connected, a server folder added as a project, its file with gutter marks, its terminal ("Server terminal", the
    server's own shell), and a project window for it. Both services were stopped afterwards; `~/qanuvi-test` (4.9 MB)
    and `~/.local/share/qanuvi` (108 KB) are left on the VPS for the owner to keep or remove.
  - Still to do: plan runs are still driven by the window, so a run stops when the desktop disconnects (moving the
    runner into the master is a follow-up); agent configuration files and the preview's inspector work on this PC only;
    the monthly budget counts this PC's sessions only; agents on the VPS need `claude` installed there; an installer or
    copy step for `qanuvi-master` from the app.

## Stage 3: personal layer

- [x] **17. Memory and rules** (done 2026-11-10): facts, decisions, solutions, approved examples, style and rules as
  separate entries; given to models as relevant slices; editable by the user.
  - Built: `memory.rs` and a `memory` table (schema 3): entries of six kinds for every project or one project, with
    tags, an id, change times, a use count and a deleted mark instead of erasing (for merging copies in Milestone 18).
    A slice for a request: the rules and style that apply (the project's own first), then the entries that match the
    request best (word matching on this PC: rarer words count more, title matches double), within a size limit
    (default 1,500 characters); nothing at all when nothing applies. It goes to the agent before the user's message
    (`with_context`), while only the user's words are stored and shown: for a new chat session, each plan step's first
    turn, and the planner. Qanuvi's tool server gets `memory_search` (more on demand) and `memory_suggest` (an
    agent's suggestion, kept as "suggested" until the user approves it) when the session's project has a memory. A
    **Memory** page: on/off and size, entries by kind with filter, new / edit / delete (confirmed), suggestions with
    Keep / Edit / Dismiss (and a count in the sidebar), and **Try it**: exactly what an agent would get for a request,
    with its size in tokens. The memory stays on this PC, also for a server's projects (their slices are sent from
    here).
  - Validated: 70 Rust tests (saving, project scope, deleted marks, suggestions waiting, slice order and budget, empty
    slice, ranking, the context before the message, the tool server's memory tools) and 108 frontend tests. In the app:
    a fact added for packet-test; Try it for "Fix the rounding of the cart total" gave just that fact, 235 characters
    (about 59 tokens); a new session asked, without reading files, how money is kept and answered from the memory
    ("integer cents, never floats, two decimals only when shown"), then suggested "Tests run with npm test" with
    `memory_suggest`, which waited as a suggestion (Sonnet, 5.8 s, $0.084). Found and fixed: the Try it preview counted
    as a use.
  - Left in the owner's app from the test: the packet-test fact and the suggested entry (packet-test is a test project).
- [-] **18. Memory disk and sync** (main part done 2026-11-10): encrypted, versioned log; local only, local plus remote, or
  remote only (Google Drive, Dropbox, OneDrive, FTP/SFTP, WebDAV, S3-compatible, own server); copies from two machines
  merge.
  - Built: `memory_disk.rs`. Each machine writes only its own file (`<device>.qxm`: its whole memory with deleted
    marks), so two machines never write the same file (which sync services handle badly); a sync reads every machine's
    file and keeps the newest version of each entry, deletions included; use counts keep the larger number; entries
    from files are checked like the user's own. Files are encrypted: an Argon2id key from the passphrase and
    XChaCha20-Poly1305 (a wrong passphrase or a changed file is refused, and then nothing is written). The previous file
    is kept as a version (`<device>-<time>.qxv`, the last 10) and can be restored (its entries come back, newer ones are
    deleted, and the other machines follow). Where: a folder (a Google Drive, Dropbox or OneDrive folder, a USB disk, a
    network share) or a folder on one of the Servers over SSH (`~/.qanuvi-memory` by default; files are written whole
    through a temporary name). The passphrase is kept in the system's credential store (Windows Credential Manager,
    macOS Keychain, Secret Service), never in a file. The Memory page has a **Memory disk** section: this computer only
    / a folder / a folder on a server, the passphrase (twice; change, forget), Sync now, sync by itself (at start-up,
    15 s after a change, every 10 minutes; main window only), the last sync (entries written, other computers read,
    what came in), and earlier versions with Restore (confirmed).
  - Validated: 76 Rust tests and 110 frontend tests: encryption (wrong passphrase, a changed byte, a new salt and nonce
    each time), two machines through a folder (adds, changes, a deletion that reaches the other machine, newest wins,
    only encrypted files on the disk, a wrong passphrase writes nothing), restoring a version, keeping 10 versions, odd
    file and folder names refused, the settings. On the owner's VPS over SSH (opt-in test): two machines synced through
    `~/qanuvi-test/memory-disk` with an add and a deletion both ways, 4 syncs in 12 s (about 3 s each, one SSH
    connection per file step); the files there start with `QXM1` and contain none of the entries' text; the test
    folder was removed afterwards. The credential store: a test passphrase saved, read back and deleted (opt-in test).
    By mouse in the app: an agent suggestion approved with Keep; A folder chosen, a test passphrase saved (it went to
    Credential Manager) and the first sync ran by itself; Sync now kept the previous file as a version; a second computer
    (simulated by an opt-in test writing its own file) was read and its entry appeared; Restore (confirmed) took the
    memory back to the earlier version and removed that entry; Forget removed the passphrase from Credential Manager;
    back to "This computer only". The test folder was removed.
  - Still to do: "remote only"; built-in Google Drive, Dropbox and OneDrive sign-in, WebDAV, S3-compatible and FTP (the
    folder option covers the first three through their desktop apps); one SSH connection for a whole sync.

## Stage 4: accounts and remote control

- [-] **19. Central Qanuvi account at qanuvi.com** (decided by the owner 2026-11-10): one Qanuvi account, signed in at
  qanuvi.com (a Qlopy site, not yet deployed) with Google, Microsoft or GitHub there; OAuth 2.0 + PKCE; devices and
  device approval, two-factor. (Qanuvi side done 2026-11-10; waiting for qanuvi.com.)
  - Found: Qlopy already has the server side (`includes/rest-oauth.php`): authorization code with PKCE S256 for public
    apps, redirect addresses matched exactly (http only for localhost), short-lived JWT access tokens and one-use
    rotating refresh tokens, revoke (RFC 7009), `GET /api/qlopy/v1/users/me`, and per-app sessions the user can end on
    Settings > API. Google and Microsoft sign-in exist in the arhamskin and direktor themes; GitHub does not; there is
    no two-factor sign-in yet.
  - Built: `account.rs` (desktop): qanuvi.com and Qanuvi's app id (`qanuvi-desktop`) are built in
    (`QANUVI_ACCOUNT_SITE` / `QANUVI_ACCOUNT_CLIENT` point it at a test copy of the site). Signing in the way apps on a
    computer do it (RFC 8252; the owner agreed): qanuvi.com's sign-in page opens in the user's browser (Google refuses
    sign-ins inside app windows), Qanuvi never sees a password; a one-time listener on `http://127.0.0.1:47213/callback`
    takes the code only with the right state; the code is exchanged with the PKCE verifier; the refresh token is kept
    in the system credential store and the access token only in memory; a new access token comes from the one-use
    refresh token when needed (a refused one signs out); "who am I" from `users/me`; signing out revokes the sign-in at
    qanuvi.com and forgets it here; Cancel stops waiting. In the window: the **Account** page (Sign in; while waiting a
    "Finish signing in in your browser" popup with Cancel; when signed in, name, e-mail and Sign out), and the account in
    the new status bar (below). At start-up the kept sign-in is checked quietly; being offline does not sign out.
  - Validated: 83 Rust tests: the PKCE challenge against the RFC 7636 example, site address rules, the authorize
    address, the listener refusing a forged state, Cancel; and (opt-in, real port and credential store) the whole flow
    against a stand-in Qlopy site that checks the verifier like Qlopy does: sign in, who am I, a restart signing in again
    from the kept sign-in with the refresh token replaced, and sign out revoking the latest token with nothing left in
    Credential Manager. In the app: the Account page and the account popup in the status bar.
  - qanuvi.com (Qlopy work, when the site is deployed): switch on Tokens (JWT) and OAuth apps; add the app
    `qanuvi-desktop` with the redirect address `http://127.0.0.1:47213/callback`; make Google and Microsoft sign-in
    (now in two themes) a part any Qlopy site can use and add GitHub; show those buttons on the sign-in page the
    authorize step uses; devices (Qlopy's per-app sessions) and device approval, and two-factor, on the site.
- [x] **19b. Status bar** (asked for by the owner 2026-11-10): a thin bar along the bottom of the window, as in VS Code:
  the account (the user's initial, or a person icon; a popup with Sign in, or name, e-mail, Account and Sign out), the
  current project's git branch with commits to pull and push and the number of changes (opens Git), servers connected
  (opens Servers; red when one failed), the memory disk's last sync or a problem (opens Memory), agents working or
  waiting, and this month's agent cost (opens Agents). The sidebar's top button is "My workspace" again.

- [x] **20. Relay service**: masters connect out; handlers reach them from anywhere; end-to-end encryption; activity log
  and "being controlled" indicator on the master. (Done 2026-11-10.)
  - Built: the relay (`qanuvi-relay`, about 680 KB on Linux; `relay_server.rs`) only passes messages on over a
    WebSocket (ws:// only for localhost, wss:// otherwise): masters connect out and register with a secret; a pairing
    code (`SLOT-XXXX-XXXX`, 10 minutes, one try) pairs a device with SPAKE2, so a relay that sees the code's slot
    still cannot learn the key; each connection gets its own keys per direction (HKDF from the pairing key and both
    sides' nonces) and every frame is sealed with XChaCha20-Poly1305 and a counter (changed, replayed or reordered frames
    are refused). On the master (`relay_client.rs`): remote access on or off with the relay address and a name,
    reconnecting with back-off, paired devices (Remove), an activity log of commands per device, Stop all. The remote
    session speaks the same master protocol as SSH servers, so every page works the same. In the window: Servers →
    "This computer: remote access" (Turn on, Pair a device with the code shown, In use now with Stop all, Paired
    devices, Show activity) and "Pair with a computer through the relay"; a paired computer is a server "through the
    relay" (its key in the credential store, removed with it); the status bar shows "Used from <device>" with Stop.
    This computer's remote access runs in its master service on the app's own data, so it keeps working while the
    window is closed.
  - Fixed while testing: a computer paired with itself listed its own projects as remote ones and Stop then emptied
    the sidebar (projects on this computer now always stay local); the Servers page could not be opened with no
    project open.
  - Validated: 89 Rust tests and 111 frontend tests: the code format, SPAKE2 pairing (a wrong code fails), sealing
    (changed, replayed and reordered frames refused), and a whole pairing and remote session through a local relay. On
    the owner's VPS: the relay ran there (through an SSH tunnel); in the app remote access connected, a code was shown,
    the same PC paired as "Test laptop" through the relay, a project opened through it (files, git), the status bar
    showed "Used from Test laptop", Stop all ended it; after the fix local projects stayed local. Then the server entry
    and the paired device were removed, remote access turned off, and the relay and its data removed from the VPS.
  - Still to do: run the relay at `wss://relay.qanuvi.com` behind TLS; tie masters to the Qanuvi account (19) so
    devices can be listed without codes; permissions per device (with 21).
- [ ] **21. Handler mode**: web handler first, then the phone app (Tauri mobile): pick master and project, commands,
  approvals, progress, diffs, screenshots, push notifications; permissions per device.

## Stage 5: assistant tools and voice

- [ ] **22. Email** (Gmail, Outlook, IMAP/SMTP), **push**, **SMS through the user's gateway**, **reminders** kept on the
  relay; drafts confirmed before sending unless a standing rule allows it.
- [ ] **23. Voice**: speech to text and text to speech in the user's language (local Whisper or a provider), from the
  phone and the desktop.

## Stage 6: local models

- [x] **24. Local coder and chat models** through Ollama or llama.cpp, with a router; cheap jobs (summaries, sorting,
  small drafts) moved to them; savings measured. Widened by the owner (2026-11-10): also free models through
  OpenRouter or OmniRoute, mixed and chosen by Qanuvi's own brain, so a developer who cannot pay for big models still
  gets AI coding. (Main part done 2026-11-10.)
  - Built: **Qanuvi agent**, a second agent next to the command-line one. `qanuvi --agent` (and
    `qanuvi-master --agent` on servers) runs in the project folder and speaks the same JSON lines, so sessions,
    approvals, history, costs, Stop and remote servers work unchanged. `llm.rs`: Ollama's own API (context set to
    16k; its default is too small for an agent) and any OpenAI-compatible API (OpenRouter, OmniRoute, llama.cpp's
    server, LM Studio, vLLM); model lists (Ollama tells which models can use tools; OpenRouter's list is cut to free
    models that can); tool calls that small models write as text are recognised. `agent_tools.rs`: Read, Write,
    Edit, Bash, Glob, Grep with the command-line agent's names and inputs, kept inside the project folder (`..`,
    other absolute paths and links refused), .gitignore respected, short answers (400 lines, the end of long command
    output), commands end after 2 minutes with everything they started. Permission modes: Manual asks before edits and
    commands, Edit automatically asks for commands, Auto asks only for risky commands (deleting, pushing, downloading,
    ...), Plan changes nothing. `brain.rs`: the user's models in their order; "Auto" sends questions to a model on
    this computer first and changes to the first model; a rate-limited or unreachable model rests (1-2 min, 30 min when
    unusable) and the next one takes over within the same turn; a model that breaks two tool calls in a row hands over;
    models that break more than a third of their calls move down for code; what it learns is kept in
    `brain.json`. The conversation is kept inside the model's context (old tool output shortened, oldest exchanges
    left out) and saved for resuming; the project's `AGENTS.md` is added to the instructions. Keys are kept in
    the credential store (`QANUVI_KEY_<PROVIDER>` on servers), never in settings or the window, and never sent
    over plain http to another machine. In the window: Agents → Qanuvi agent (models in order with what the brain
    learned, Up/Down/Remove; per provider its address, key and Find models; add any OpenAI-compatible server); the
    session shows when the brain hands a turn to another model.
  - Validated: 106 Rust tests and 115 frontend tests, among them the tools (folder limits, edits with Windows line
    ends, ignored files, commands, risky commands), the brain's order, rests and distrust, the context trimming, and a
    whole turn against a stand-in model server (a rate-limited model skipped, Read, Edit after approval, the answer,
    the brain's records and the saved conversation). Live on this PC (RTX 3050 Ti 4 GB, Ollama, qwen2.5-coder:3b):
    from the command line a rename in a Python file in 10 s; in the app Find models listed the local model (tools, 16k,
    1.9 GB) and OpenRouter's free tool-using models, the model was added, and a question in a session was answered
    with Read and Grep in 2.7 s, 2.4k tokens in and 83 out, $0. By mouse: Refuse (the file stayed as it was) and
    Approve (notes.txt got its new last line; 2.6k tokens in, 108 out, $0).
  - Fixed while testing: the small model called itself another company's assistant (the instructions now name the
    Qanuvi agent and the model it runs on, and it answers so); a tool call written as text to a tool that does not
    exist was shown as the answer (now refused, so the model tries again); edits that cannot work (text not in the
    file, not unique, no change) are answered at once instead of asking the user; the approval card shows what an edit
    replaces and with what, and that a write replaces the whole file (for both agents).
  - Free AI from the start (asked for by the owner 2026-11-10: a new user with no paid plan gets workable AI help):
    - When no paid agent is found, the session pane offers "Set up free AI" instead of a dead end; once free models are
      set up, new sessions use the Qanuvi agent by themselves.
    - Step-by-step guides (`guides.ts`) for Ollama, OpenRouter and OmniRoute, researched and dated 2026-11-10:
      pages to open, commands to copy, the key, Check key (OpenRouter: free requests left today), Find models, Use the
      recommended ones. Facts used: OpenRouter's free models give 50 requests a day (1,000 after buying $10 once), 20 a
      minute, some need the privacy setting for free models; OmniRoute (MIT) installs with npm, runs on port 20128, its
      key is under Endpoints, auto/coding routes between free providers.
    - The two local models of the plan, fitted to the machine (`machine.rs` reads memory and the graphics card): a
      coder and a conversation-and-thinking model from a dated list of Ollama models that can use tools (from
      qwen2.5-coder:1.5b up to qwen3-coder:30b; qwen3:1.7b up to qwen3.8:27b), fast when it fits the card, slower from
      main memory otherwise; "Get it" downloads through Ollama with progress and adds it. On the owner's PC (15.3 GB,
      RTX 3050 Ti 4 GB): qwen2.5-coder:3b and qwen3:4b.
    - Free cloud models ranked by rules (must use tools and be free; coding, bigger and longer-context models first;
      tiny ones left out; OmniRoute's auto/coding first, OpenRouter's free router last; turns finished here count).
      Live: 14 free OpenRouter models with tools; recommended nemotron-3-ultra-550b, gemma-4-31b, then openrouter/free.
    - Update with a paid agent: with the user's yes, Claude Code checks the pages on the web and answers with the guides
      in the same shape; Qanuvi checks them (known providers, https pages, one-line commands, a date), shows what
      changed and keeps them only when the user accepts; Use the built-in ones goes back.
    - Standing approvals (both agents): an approval card also offers "for this session" and "for 10 minutes"; file
      changes count as one kind, other tools each their own; risky commands always ask. Kept by the session manager in
      Rust, so plan runs and remote masters follow it too; "Ask again" above the input takes them back. In the models
      list "in use" became "Stop using", and "Remove all" empties the list.
    - Live on OpenRouter with the owner's key (2026-11-10): key check showed 50 of 50 free requests; a two-edit change
      went to nemotron-3-ultra-550b:free (3 tool calls, none broken, $0); the second edit was approved by "for this
      session".
    - The model at work is shown: the brain names its pick and the reason at the start of each turn ("A question:
      ollama/qwen2.5-coder:3b on this computer answers it, which costs nothing."), the "Working…" line and the line
      under the conversation name the model, handovers name the next one.
    - Streaming: Ollama (JSON lines) and OpenAI-compatible servers (data: events, tool arguments joined from pieces,
      usage from the last event; a server answering in one piece still works). Text shows as it is written and the
      whole message takes its place; text that is a tool call written as JSON is not shown live. Partial text goes to
      the window only, not into the stored session. Live: 58 pieces from qwen2.5-coder:3b; nemotron-3-ultra on
      OpenRouter seen mid-sentence in the app.
    - Seen: the brain sorts jobs by words only, so "Read cart.txt and write a paragraph… do not change any file" counted
      as a change (went to the cloud model, not the local one). For 24b.
    - Local servers found and started (local_servers.rs): when Find gets no answer from Ollama or OmniRoute, Qanuvi
      looks for the program (PATH, then the usual install places), starts it hidden in the background (`ollama serve`,
      `omniroute serve --daemon --no-open --port N`), waits until it answers, then lists the models; not installed:
      it says so and opens the install steps; running but asking for a key: it says so and opens the steps. Live:
      OmniRoute 3.8.52 (npm) found and started on the owner's PC, no window, then asked for its key.
    - Connect Qanuvi (OmniRoute): OmniRoute's own command line (it signs in with this machine's local token) makes an
      API key named "Qanuvi"; Qanuvi keeps it in the credential store, never shows it, then finds the models. Models
      behind OmniRoute count as cloud models, not "this computer" (the brain sends questions to local models first);
      image, audio and embedding models are left out. The free-model ranking knows frontier families whose names carry
      no size (Claude Opus/Sonnet, Gemini Pro, GPT-5: first; Gemini Flash, Kimi, GLM, DeepSeek, Qwen3 coder: strong).
      Live on the owner's PC: connected; Antigravity offers Claude Sonnet 4.6, Claude Opus 4.6 Thinking, Gemini 3.8
      Flash and GPT-OSS 120B; Kiro and Kimi Coding sign-ins had run out.
    - Only connected models: OmniRoute's OpenAI-style list named 342 models (most from providers not connected);
      Qanuvi keeps the ones OmniRoute's own list marks available, plus its routes and combos (49). Then each of the
      six best-ranked is tried with one tiny message (with one tool) before it is recommended; those that do not
      answer are struck out with the reason. Live: auto/claude-opus and auto/claude-sonnet failed (500 from a provider
      behind them); Claude Sonnet 4.6 (6.7 s), Claude Opus 4.6 Thinking (6.2 s), auto/coding (4.4 s), GPT-OSS 120B
      (4.2 s) answered. An edit in git-test through OmniRoute's Claude Sonnet 4.6: read, edit, approved, done, $0.
    - Code-index tools for Qanuvi's own agent: project_map, find_symbol, outline, read_lines, search, called in the
      agent's process (no MCP server), read-only (no approval, allowed in plan mode); the instructions prefer them over
      reading whole files. Live: qwen2.5-coder:3b found grant_minutes with one find_symbol call (2.4k tokens, 8 s).
    - Approval cards by agent (owner, 2026-11-11): Qanuvi agent: Approve · Allow for N min (N set on the Agents page,
      1–240, default 10) · Decline; a paid agent (Claude Code): Approve · Allow for this session · Decline. Slim
      buttons on one line, Decline quiet at the end. Live: "Allow for 10 min" on the first edit, the second went
      through by itself. "Qanuvi brain" is called "Qanuvi core" in the app.
    - Seen: the agent's answers show markdown as plain text (**bold**); render it in the pane.
    - Free models first in plan runs: when the run's agent is paid and Qanuvi core has models, each step (money and
      security work and steps naming a model excepted) first goes to Qanuvi's own agent ("qanuvi:auto", shown as
      "Qanuvi core (free)"); when that does not finish the step, the paid agent takes over from the model it would
      have used, and climbs as before. Run option, on by default. The run panel shows the steps free models finished
      and what they saved, measured from the user's own runs (average cost of a finished paid step of the same kind;
      this run's paid steps when there is no history). Live (free-test, 4 steps): README, add, sort on Qanuvi core
      (OmniRoute Claude Sonnet 4.6) $0; the price step went straight to Claude Code Sonnet 5.5, $0.05; about $0.12
      saved; all files right, all steps marked done.
    - Crash fix (2026-11-11): the owner's laptop display went black then grey. Windows logs: Qanuvi's WebView2 grew
      5.4 → 8.9 GB in two minutes (with Photoshop 6.6 GB on 15.3 GB RAM), the desktop compositor ran out of memory
      (DWM exit 0xc00001ad ×6), disk paging errors, forced restart. Cause: the page's project effect called
      agent.loadProject → newSession, which read agent.providerId right after writing it, so the effect depended on
      it; restoring the last session (step 4 on Claude Code, default agent Qanuvi) set it back, the effect ran again —
      an endless loop, ~50,000 DOM nodes a second. Fixed in both places (newSession reads settings only; the effect
      runs its work untracked). Verified: nodes steady at 3,665, window ~200 MB. Not the local model (Ollama ~150 MB).
    - Qanuvi core records: sessions save brain.json by adding their own changes under a lock file (two sessions at
      once used to overwrite each other); tested with 8 threads.
    - Markdown in the agent pane (agent/markdown.ts, small and safe: escaped first, then headings, bold, italics,
      code, lists, quotes, http(s) links opened in the browser); text being written stays plain.
    - Local servers: Ollama is the local engine (it is llama.cpp inside), so no separate llama.cpp. Other servers on
      this computer (LM Studio, plain llama.cpp, vLLM) can have a start command that Qanuvi runs when they do not
      answer. OmniRoute now starts hidden (`serve --no-open --no-tray`; `--daemon` opened a console window). The agent
      starts a stopped Ollama/OmniRoute itself mid-turn and asks the same model again ("omniroute is not running;
      Qanuvi starts it.").
    - Load measured (owner's PC, 2026-11-11): Qanuvi idle ~200 MB (window ~185–210 MB + 8–15 MB core), CPU ~15–20%
      (the machine's own idle). Local qwen2.5-coder:3b: graphics memory 0.5 → 2.8 GB (stays loaded 15 min), RAM −1 GB,
      GPU/CPU peaks ~70–76% for seconds. OpenRouter: no local load beyond Qanuvi. OmniRoute: ~400–560 MB RAM. All
      three together: free RAM 7.3 → 5.9 GB. Seen: qwen2.5-coder:3b read only line 1 of cart.txt and guessed the rest.
    - Room check before anything heavy (owner, 2026-11-11: "measure the host PC before running anything"):
      machine::room() reads free memory and free graphics memory right then; a local model loads only when it fits
      (its size + context; the card takes what it can, the rest must fit in memory with 1.5 GB left for the computer),
      otherwise the next model takes the turn with the reason in numbers. Starting Ollama/OmniRoute checks the same
      way. A model already loaded needs nothing more.
    - Online only (owner): "Use models on this computer (Ollama)" on the Qanuvi agent panel; off = no Ollama card, the
      OpenRouter guide opens first for new users, local models in the list are skipped, nothing is installed, loaded
      or started here. Live: off → a question went to Claude Sonnet on OmniRoute ("no model on this computer is set
      up"), right answer.
    - Logo v6 (purple Q, green braces, red dot) everywhere: taskbar/app icons, title bar, favicon.
- [x] **24 done (2026-11-11)** apart from what moved to 24b (the decider) and 24c (first-time setup).
- [-] **24c. First-time setup from Qanuvi's own knowledge (owner, 2026-11-10; wizard and OmniRoute done 2026-11-11, remote knowledge built 2026-11-11 on the owner's server, moves to qanuvi.com later)**: on first run Qanuvi
  offers to set up free AI by itself, from what it knows (like the OpenRouter and OmniRoute setups: find what is
  installed, start it, connect it, pick the best free models), with the user's yes at each outside step. That
  knowledge (guides, model lists, rankings) is later updated remotely from qanuvi.com; how is to be planned.
  - Owner's wizard (2026-11-11): one guided setup so the user configures nothing by hand.
    - Local: measure the PC; if it fits, download and install Ollama itself (with Windows' admin prompt when needed),
      pull the fitting coder and chat models, start and connect them. Tell the truth when the PC is weak: small
      local models answer questions but are not good enough for real coding — say so and steer to online models.
    - OpenRouter: open the sign-up/sign-in page in the browser, wait for the user, then take the key (pasted into the
      wizard) and set everything up (check key, free models, test messages, order).
    - OmniRoute (owner said yes, 2026-11-11): install if missing (npm), start hidden, make Qanuvi's key, connect
      API-key providers itself (reusing the OpenRouter key), build a free fallback combo, watch sign-ins and quota
      and warn before they expire; sign-ins (Antigravity/Google, Kiro, Kimi) open in the browser for the user and
      Qanuvi does the rest (`omniroute oauth start` — to be tried first). Advanced users can turn the assistance off
      and use OmniRoute's panel. Each outside step only after the user's yes.
    - Remote knowledge from qanuvi.com: provider steps, free-model lists, rankings, OmniRoute commands, published
      by the owner and fetched by each Qanuvi when newer (signed, checked like the guides are now; shown before use).
  - Built (2026-11-11):
    - OmniRoute run by Qanuvi (omniroute.rs, OmniRoutePanel): connections with health (working / needs sign-in /
      resting / error, from OmniRoute's provider status), Test, Renew sign-in (Antigravity: OmniRoute's own login
      helper, which catches Google's answer itself; others open OmniRoute's own page), a curated list of free
      providers (Antigravity, OpenRouter, Gemini AI Studio, Groq, Cerebras, NVIDIA NIM, Mistral; dated, for qanuvi.com
      to update) connected with a pasted key or the key Qanuvi keeps (sent on stdin, never on a command line), the
      whole catalog (371 providers) for experts, and the "qanuvi-free" fallback combo built from the models that
      answered Qanuvi's test (then put in Qanuvi core's list). "Let Qanuvi run OmniRoute for me" can be turned off.
      Live: connections read right (Kiro and Kimi need sign-in), OpenRouter tested, qanuvi-free made (Sonnet 4.6 →
      Opus 4.6 Thinking → GPT-OSS 120B) next to the owner's own combos, untouched.
    - Setup wizard (SetupWizard): this computer (memory, card, free now) with an honest verdict (localVerdict: "for
      questions" on small cards, "for coding" with a capable coder that runs fast, "none"); ticks for local /
      OpenRouter / OmniRoute; installs Ollama, Node.js (winget, Windows asks itself) and OmniRoute (npm) at a click
      (other systems get the official command), downloads the fitting models, takes the OpenRouter key and tests the
      free models, starts and connects OmniRoute, reuses the OpenRouter key there, tests and makes the combo; ends with
      online models first and the Qanuvi agent chosen. Opened from "Set up free AI" and from the Agents page. Live on
      the owner's PC (all installed): every step ran, 11 models, window ~220 MB. Not yet tried live: the installs
      themselves (nothing missing here) and the Google sign-in.
  - Remote knowledge built (2026-11-11): one signed JSON file (guides, free providers) published by the owner at
    https://qlopy.top/qanuvi-updates/knowledge.json (the owner's own web server, until qanuvi.com), with an Ed25519
    signature next to it. Qanuvi checks once a day at start (and on "Check for newer"), refuses anything not signed by
    the built-in key or not well formed, shows what changes ("Newer knowledge from Qanuvi": version, date, note, guide
    and provider changes) and uses it only after "Use it"; "Not now" stops the daily offer for that version; "Use the
    built-in ones" goes back. The OmniRoute panel's free providers come from it. Publishing: qanuvi-knowledge
    (keygen / sign), steps in docs/knowledge/README.md; the private key is kept outside the repository and the web
    folder. Live: version 1 offered at start and kept; a file changed after signing was refused with the reason.
- [ ] **24d. More free providers, images and video (owner, 2026-11-11, later)**: other big free sources besides
  OmniRoute and OpenRouter (free Claude/Gemini through sign-ins like Antigravity: Kiro, Amazon Q, GitHub Copilot
  Free, Gemini CLI; Kimi models where a free tier exists), and image and video generation: free tiers (Pollinations,
  AI Horde, a local Stable Diffusion WebUI; video is mostly paid) and paid APIs the user connects (fal.ai, Runway,
  Leonardo, Ideogram, Recraft, MiniMax). Unofficial "web" bridges are left out by default (they break and may break
  the site's terms).
  - Checked on the web 2026-11-11 (to recheck before building): no big provider gives Kimi K3 free — Moonshot's API
    starts after a small top-up, OpenRouter lists K3/K3.1 Pro paid only; free K3 claims come from small unverified
    sites. Free big open models do exist: Ollama Cloud (Kimi K2, GLM, Qwen3, gpt-oss; one request at a time, session
    caps), NVIDIA NIM (many models, evaluation limits), ModelScope (~2,000 calls a day), Gemini AI Studio (Gemini Flash
    free), Mistral (Codestral, Devstral; data may be used for training), Cloudflare Workers AI, Groq, Z.ai GLM; no
    free Claude API anywhere (only through sign-ins like Antigravity or Kiro). Kimi K3.1 Pro weights are open (too
    big for a home PC).
  - Small (2026-11-11): sign-in rows show the account to sign in with; "Test" on a sign-in connection tests one of its
    models (OmniRoute cannot test those and said "SKIP", which used to read as a pass).
- [x] **Release for self-development (owner, 2026-11-11)**: after 24b, a release build (installer) the owner uses to
  develop the rest of Qanuvi with Qanuvi itself. Built 2026-11-11: Qanuvi_0.1.0_x64-setup.exe (NSIS only, 5.6 MB;
  `npx tauri build`, output in src-tauri/target/release/bundle/nsis). Code on github.com/hmsiddiki/qanuvi.
- [x] **24f. Plan runs that finish by themselves (owner, 2026-11-11; built and tested 2026-11-11; the task queue is later)**: Qanuvi becomes an assistant with its own
  task runner that keeps working on what the owner gives it, even when the owner is away. Today a run goes from the
  chosen step to the end without pausing, but the whole run stops at the first blocked step, at the budget, or when a
  step fails on every model (after the planner's rewrite).
  - How it pauses (the user picks): never (runs to the end), after every step, or only where a decision or the user's
    input is needed.
  - Which steps: all open ones, from a step, or ranges such as "2-6, 10-15".
  - Dependencies: a step can say what it needs ("- Needs: 3", or a step in another plan). A step whose need is not
    done, or that is blocked, is skipped with the reason written under it, and the run carries on with the rest
    (configurable: stop instead).
  - Every step finishes with its checks: the project's tests and the decider's review (24b/24e); what fails goes to
    a stronger model, then to the paid agent, then is skipped with the reason.
  - At the end: a report (done, skipped and why, blocked and why, cost) in the run panel and the Inbox; the skipped
    and blocked steps are easy to run again once their reason is gone.
  - Later: a task queue for the assistant (tasks the owner assigns, run one after another, also on a remote machine
    through the masters), with the paid agent on top as the monitor (24e).
  - Built (2026-11-11):
    - Run panel: "Steps" (ranges such as 2-6, 10-15; unreadable ranges are named and Start waits) and "Stop for me"
      (Never, the default / Only where a decision or my input is needed, as before / After every step).
    - "- Needs: 3, 5" or "- Needs: OTHER-PLAN.md#4" under a step; a step whose needs are not done is skipped, "-
      Skipped: why" is written under it (removed when a later run finishes it), and the run carries on.
    - With "Never": a blocked step (with the agent's own "Blocked:" reason) or one no model could finish is left with
      its reason and the run goes on; steps that need it skip themselves.
    - The end of the run reports what was done, skipped, blocked and failed, with why, in the run panel.
  - Live (free-test, 5 steps, "Never"): 1 and 2 by free models (reviews accepted), 3 blocked by the free model (it
    would not invent the shop's name, as told), 4 skipped ("needs step 3, which is not done", written under it), 5 by
    Sonnet; "3 done · 1 skipped · 1 blocked", $0.08 with the decider's $0.05. "After every step" with range 1-2:
    paused after step 1 ("Run again to go on").
  - Fixed while testing: the note for the paid agent (24e) showed in the user's message after a reload.
  - The report also goes to the Inbox (see "Inbox" above).
- [x] **24b. The decider (owner's design 2026-11-10, revised 2026-11-11; built and tested 2026-11-11)**: one model takes the
  decisions (what kind of job, which model does it, when to hand over, whether the result is good enough), the others
  do the work.
  - Who decides: if the user has any paid agent, it must be the decider. With no paid agent, the free models decide,
    best first, one after another (the next takes over when one is unavailable).
  - Modes (the user picks one):
    1. **Paid decides, free works**: the paid model decides; free models do the work; the paid one does what the free
       ones could not, or steps in when they get into trouble (Qanuvi core watches: broken tool calls, failed tests,
       repeated errors, a step not finished).
    2. **Priority**: save tokens / balanced / solve problems first (bigger plans spend more on the paid model).
    3. **Direct**: only the paid model, as today; Qanuvi core helps with context (memory, packets); MCP tools stay the
       model's own feature.
    4. **Free decides, paid works** (for chat): a free model decides what to send to the paid one (which model,
       how much context), so the paid one gets fewer, better requests.
  - Quality first, cost second: cost control is a choice, quality always matters. A free model's result is checked
    before it is accepted (the project's tests where there are any; the decider reviews the change — a short look at
    the diff, not a re-run); not good enough goes to the paid model.
  - Small paid packages: GitHub Copilot (its $10 plan and its bigger ones) is a must, later; Claude Code first.
  - Today's brain (question/change by words, local first for questions, list order for changes, handover on rate
    limits, outages and broken tool calls, records per model) is the fallback whenever the decider is unavailable or
    answers nonsense.
  - A model the user picks is the only one used (owner, 2026-11-11): no rotation, no handover, whatever else is set;
    when it fails, the error says why and the pane offers "Switch to Auto".
  - Built so far (2026-11-11):
    - src/lib/core/decider.ts: modes, seats (decider/workers; a personal model or Copilot can take a seat later),
      the plan assignment prompt and its parser (money and security always paid; unreadable answers fall back to
      rules by priority), the review prompt (anything unclear = not accepted), the chat model choice, decision records.
    - Rust: brain::decide asks Qanuvi core's free models one after another for a short decision (no tools);
      core_decide command; requests without tools leave the tools field out.
    - Plan runs: with a paid agent and free models, the decider (the paid agent, read-only, one call) assigns every
      step to free or paid; a free model's "done" is reviewed by the decider against the step's diff and the tests;
      rejected → the step reopens and the paid model gets it with the reason; decisions kept in the run record.
      Direct and Free-decides modes keep free models out of plan runs.
    - "How the team works" panel on the Agents page (four modes, priority).
    - Fix: a model or permission mode chosen mid-session now reaches the running agent (set_model /
      set_permission_mode, the same requests the CLI agent takes); choosing on the Agents page also applies to the
      current session. Live: gemma chosen → "you chose it", provider busy (429) → no rotation, "Switch to Auto" → the
      same session went on with the local model.
  - Built next (2026-11-11):
    - Free decides, paid works (chat): the paid agent's model menu has "Auto (a free model picks)"; per message a free
      model picks one of the paid agent's own models (generic: whichever paid agent the user has), and the pick and
      its reason show in the conversation.
    - A troubled free chat (Qanuvi agent on Auto) goes to the paid agent with what the free model did: automatic in
      "Paid decides" and "Priority", a "Hand this to ..." button otherwise.
    - The run panel shows the decisions (assignments and reviews).
    - Command menu: "/" opens it, any other key closes it, picking closes it, the agent chip opens it.
    - Fix: the review saw changes from earlier steps (diff against the last commit), so a correct step 3 was
      rejected. The runner now keeps the files' text before each free step and the review sees only that step's lines
      (stepDiff, with tests).
  - Live tests (2026-11-11):
    - Menu: "/" opens; a, Right, Left, Esc, Up, Enter and 1 each close it; Enter sends nothing; chip opens it.
    - Free decides: a free model picked Haiku 5.5 for a simple file question; answer correct, $0.0042.
    - Plan run (paid decides, free-test, 4 steps): steps 1-3 free, step 4 paid (money); every free step reviewed and
      accepted first try after the fix; 4/4 done, $0.03; Qanuvi + WebView at most 250 MB, free RAM never under 4.5 GB.
    - Not tested live: the automatic hand-over (the free models did not fail any request in the tests).
  - Moved to 24e: plan runs always go to the paid agent when one is installed.
- [x] **24e. Paid agent on top, in every mode (owner, 2026-11-11; built and tested 2026-11-11)**: when a paid agent is active (installed, signed
  in, not at its limit) it is always the top layer: the decider and the overall monitor that keeps everything on track.
  Free models may work and even take a first pass at deciding, but the paid agent checks and can overrule.
  - Plan runs: always the paid agent as decider, whichever agent is chosen for new sessions (today a run with the
    Qanuvi agent chosen is free-only and nobody reviews).
  - Free decides, paid works: the free model stays the first layer (picks the model, trims the request); the paid
    agent sees that pick with the request and can overrule it (ask for a bigger model, or say the trimmed request lost
    something). Overrules are recorded, so the free picks get better and the user can see how often it happens.
  - Free chat (Qanuvi agent on Auto) with a paid agent present: the paid agent reviews free models' changes (not
    questions) before they count, as in plan runs; trouble hands over as today.
  - The monitor: one short, read-only paid call at the checkpoints (a step done, a change made, trouble seen, a free
    pick), never per message for plain questions; its notes go into the decisions list.
  - When the paid agent is not active (limit, sign-in expired, offline): the free models take the top seat one after
    another, as with no paid agent, and hand it back when the paid one returns; the pane says so.
  - Direct mode: unchanged (only the paid agent works). No paid agent: unchanged (free models decide).
  - [x] Plan runs and plan writing use the paid agent whenever it is active (agent.runProviderId), whichever agent new
    sessions use (2026-11-11). Live: Qanuvi agent set for new sessions, run of step 1 on free-test: Claude Code
    assigned it, a free model did it, Claude Code reviewed and accepted it.
  - [x] The decider's own calls count in the run's cost, as their own line ("Decider (Sonnet 5.5)"), and the
    savings line says what the checks cost. Reviews use the agent's own default model (Sonnet for the owner), not the
    planner's bigger one; the one assignment per plan stays on the planner model.
  - [x] Free decides, paid works: the message to the paid agent carries the free pick and its reason; the paid agent
    can answer "NEEDS_STRONGER_MODEL: why", and the same message goes again, in the same session, one model up, with
    a line saying so (not when the pick is already the strongest). Live: two questions, Haiku picked, answered, no
    false overrule. The overrule itself is covered by tests, not triggered live.
  - [x] Free chat (Qanuvi agent on Auto, mode "Paid decides" or "Priority") with a paid agent present: the changed
    files are noted before the turn; when it ends well and changed something, the paid agent reviews only that turn's
    change (quietly: the pane keeps the conversation). Accepted: a line with the reason and cost. Not accepted: the
    paid agent takes the request over. Live (git-test): a free model added a line, Claude Code accepted it ($0.047).
  - [x] Paid agent cannot answer (limit, sign-in, offline): the assignment and the reviews go to the free models, one
    after another, and the decisions say who decided and why; the next decision tries the paid agent first again.
  - Fixed while testing: a quiet decider call took over the agent pane (the planner selected its session).
