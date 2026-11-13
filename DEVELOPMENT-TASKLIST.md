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
  - 24u part 2 built and driven live 2026-11-14 (see Part 2 below). RESUME HERE (new conversation): ask the owner
    about the part 2 finding (a closing step done in an earlier run is not run again when a later run finishes the
    steps it had set aside), then part 3 (sorted asks), part 4 (central Inbox with answer forms, Windows
    notifications). Each part in a short session; drive it live on a scratch project only. After 24u:
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
    - Finding for the owner (not changed): 1z was done in run 1, so run 3 did not run it again after 1a/1b and the
      README still lists only add. Proposal: keep "closing step ran while X was set aside" per plan, and a later run
      that finishes X opens that closing step again (the same re-run as within one run).
  - Part 3, sorted asks: the check sorts each risk and question into (a) decides itself: a sensible default that
    can be changed later (a setting, not hard-coded), logged "Decided for you: X, because Y. Change?"; (b) goes ahead
    with extra checks: the paid decider reviews the diff and tests must pass before the step counts as done; (c)
    truly needs you: secrets, actions on outside accounts or anything leaving this computer. Risk is judged by where
    the step runs (money on a test database is not real money). Each item comes with the recommended answer chosen,
    plus "Accept all recommended"; holds only where the owner chose one or (c).
  - Part 4, one notification centre: the Inbox moves into Qanuvi's database (Rust side), seen by every window, the
    runner and later the phone. Types: for your information, needs an answer, problem. Answer items carry their form
    (options, text, "Let Qanuvi decide", "Skip this step"); the first answer anywhere closes it everywhere. Settings,
    Notifications: per type, Inbox (always), Windows notification, phone; bundled ("5 things need you"); quiet
    hours. Clicking a Windows notification brings Qanuvi forward and opens the same answer window used in the
    Inbox. Secrets are answered in the main app only. The phone is a delivery target in the design and settings;
    the phone app itself (through the Milestone 20 relay) is its own later step.
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
    Rust test in sessions_are_stored_renamed_listed_and_deleted. Not driven live yet.
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
- [ ] **24k. Master OmniRoute (owner, 2026-11-12, later)**: OmniRoute is said to reach about 300 models with about 1.6
  billion free tokens a month. Learn its providers, limits and sign-ins; automate as much of the setup as possible
  (connections, keys, combos, health); keep what is learnt in Qanuvi's brain (the signed remote knowledge, 24c) so
  every Qanuvi knows it. Headroom can sit in front of it for the free models too (`--openai-api-url`), to be tested.
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
