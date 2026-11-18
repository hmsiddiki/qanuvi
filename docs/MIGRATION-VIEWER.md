# Migration plan: the app is a viewer of masters

Status: AGREED by the owner (2026-11-17), all decisions settled. Next: M0, then M1, in a fresh session.
Going back: branch `old-infrastructure` / tag `v0.1.23-old-infrastructure` (see OLD-INFRASTRUCTURE.md there), data
backup at `E:\Backups\qanuvi\old-infrastructure-0.1.23`.

## The goal (owner decisions 2026-11-16 and 2026-11-17)

- A master is any machine running Qanuvi: a VPS, a laptop, another desktop, or This PC itself.
- The master is Qanuvi. It owns projects, files, agents, plans, Inbox, memory, settings, its account and its own
  server list. It is fully usable from a terminal with no display; in a terminal the agent does the work (paid or
  free models, as designed today).
- The app (the interface) is the visual container. It shows one master at a time, and the same screen for every
  machine.
- This PC runs its own master, and the interface shows it by default.
- A master can connect to other masters. The interface shows them nested under that master in the switcher.
- Nothing on screen is shared with This PC.

## The split (agreed)

| Side | What |
|---|---|
| **Master** | account, projects, files, git, terminals, agents, plans, Inbox, memory, settings, server list, backups, start-up list, usage, model keys, decider, web pages/images for attachments, knowledge sources; **two pipes** lent to a viewer: run a program and pipe its input and output, and forward a port |
| **Interface** | editor, chat, voice, layout, windows, popup notifications, app updates, this screen's file picker and clipboard; **language servers, debugger, inspector/preview** |

The terms used in that split:

- **Coding tools are the interface's.**
  - Language servers (Intelephense, TypeScript, Svelte, rust-analyzer, Pyright), debuggers (js-debug, PHP Debug)
    and the preview belong to the interface.
  - They run on the machine that has the files, because they need its files, packages and toolchain. The interface
    starts them there through the master's pipes and speaks their protocols itself. The master knows nothing about
    LSP or debugging.
  - They are optional on each machine. They are installed on the shown machine only after the user's yes:
    - npm servers go into that master's data folder;
    - rust-analyzer goes through that machine's rustup;
    - a server already on that machine's PATH is used.
  - The master never needs them for its own work. A terminal-only master runs with or without them.
- **Small machines.**
  - Nothing starts by itself; a tool starts only when a file of its language is opened on screen.
  - Tools end when the viewer leaves.
  - Each server's memory need (`need_gb` in lsp.rs) is checked against the machine's free memory before starting.
    If it does not fit, the screen says so and does not start it.
  - A per-machine switch keeps coding tools off.
- **The account belongs to the master.** Account memory follows the account, so every machine signs in for itself.
  - **Terminal or VPS:** `qanuvi-master login` with the username and a Qlopy **application password**. The core
    already has these (`qp_app_password_create`, password grant at `/api/qlopy/v1/auth/token`). The master keeps the
    application password encrypted in its secret store and signs in again by itself when the 30-day session ends.
    So it lasts until the user revokes that password on the site.
  - **With the interface open:** today's browser sign-in, handed to the shown machine.
  - **Site side:** qanuvi.com needs a "Machine passwords" page for normal users, as a plugin or theme page; today only
    `admin/api.php` creates them and it needs `edit_posts`. "Application passwords" must be on in Settings > API.
    There is NO Qlopy core change. Device code sign-in is dropped (not needed).

## Where we are (measured at 0.1.23)

- **Two cores, two dispatchers.**
  - The app's built-in core: `desktop.rs`, 212 commands.
  - The master: `master.rs` `Master::dispatch`, 175 commands.
  - They duplicate the wiring for the Inbox, plan runs, background loops and shutdown, and no test compares them.
- **49 commands only the app answers. Under the split:**
  - Stay in the interface: `notify_*`, `app_info`, `app_restart`, `startup_path`, `*_kill_window`/`*_stop_window`,
    `inbox_import`, `plan_data_import`, `data_location_*`, `data_test_build`, `attachment_file`.
  - Interface, running through the pipes: `lsp_*`, `dap_*`, `inspector_*`, `browser_agent_shot`.
  - Move to the master: `account_*`, `remote_*` (each core connects onward), `core_decide`, `knowledge_fetch`,
    `attachment_page`, `attachment_image`.
- **A shown master gets no coding tools today.**
  - Language servers are silently off for master projects (`lsp.svelte.ts` `#root`).
  - `dap_*` fails with "cannot do this yet".
  - The inspector always browses This PC's localhost.
  - The owner postponed this on 2026-11-15; this plan brings it in M1.
- **The window routes calls** (`src/lib/remote.ts`).
- **The server list lives in window localStorage.** No master connects onward.
- **This PC has no master of its own,** apart from the reduced remote-access one (`mark_app_data`).

## Phases

Each phase ends with verify green, a live check on scratch projects, and the owner's review. Releases only with the
owner's yes.

### M0. Guard rails

- A parity test: every command the app answers is either answered by the master or is on the interface list.
- It starts with the known gaps listed, and each phase removes some.
- BUILT 2026-11-17: `master.rs` test `answers_every_window_command_or_it_is_the_interfaces`. It reads the app's
  `generate_handler!` list and the master's match arms (`dispatch`, `inbox`, `plan_run`). `INTERFACE` lists 35
  commands that stay with the screen; `GAPS` lists 14, each with its phase (account_* M2, remote_* M3; core_decide,
  knowledge_fetch, attachment_page, attachment_image by M6). It also fails when a listed gap is already answered or a
  listed command no longer exists, so the lists cannot go stale.

### M1. Coding tools on a shown master (ships on its own)

This PC is untouched in M1.

- **Master pipes:**
  - `pipe_start` / `pipe_send` / `pipe_stop` run a program on that machine with stdio streamed over the
    connection.
  - `port_forward` opens a local port on the viewer's side that reaches the master's localhost port.
  - Both end with the connection.
  - BUILT 2026-11-17 (M1a, `pipes.rs`):
    - Master side: `pipe_start` (program, args, project folder or home) / `pipe_send` / `pipe_stop`, and
      `port_open` (port on 127.0.0.1 or ::1) / `port_send` / `port_close`.
    - Bytes go as base64, untouched. Events are `out`/`err`/`exit` for a program and `data`/`closed` for a port.
    - Every send carries a `seq`, and the master writes in that order, because it answers each command on its own
      thread.
    - Pipes are closed with the connection, as terminals are.
    - Viewer side: the `port_forward` / `port_unforward` app commands (interface list) listen on a free 127.0.0.1
      port and carry each socket through `port_open`. Sends don't wait for an answer (`Connection::tell`).
      Forwards end when the master disconnects.
    - Remote streams now hold a `Sink`, and a stream nobody listens to any more is forgotten.
- **Generic machine questions** for installs: whether a program or file exists there, free memory, and running an
  install command with live output. The master already does this for setup installs.
- **Language servers on a shown master:**
  - Remove the `#root` block.
  - Status, install and start happen on the shown machine through the pipes, after the user's yes.
  - The memory check uses that machine's free memory.
  - A per-machine "coding tools off" switch.
  - The screen says why when a tool is missing or does not fit.
- BUILT 2026-11-17 (M1b), language servers:
  - The master answers generic `host_*` questions (`host_info`, `host_find`, `host_exists`, `host_run`, and
    `host_write_new`, limited to its data folder); free memory comes from `machine_room`.
  - `lsp.rs` has a `Host` trait: `Here` is This PC; `On` is a master through its connection. Status, install and
    the memory check run against the file's machine.
  - `Servers::start_on` runs the server through `pipe_start`, and the framing stays in the app.
  - The app's `lsp_statuses` / `lsp_install` / `lsp_start` take `master`, and `lsp_*` are window-only in remote.ts.
  - The screen side:
    - Statuses are kept per machine and the `#root` block is gone.
    - The install offer is tied to the file's machine.
    - `languages.toolsOff` is a per-machine switch ("Keep coding tools off on …").
    - A machine leaving the screen ends its servers (`park` → `lsp.stopMachine`).
    - A connection that ends drops its streams, so its servers report that they stopped.
  - Driven live through the app window on the local test master 2026-11-17 and on the VPS (after Node.js was put
    there; before, the screen said why each server could not run).
  - Leftovers DONE 2026-11-17:
    - Live install output: `host_run` sends each output line as it comes when the caller listens (`onEvent`), on
      This PC and on a master; npm runs with `--loglevel=http` (without a terminal it says nothing until it ends).
      The window shows the newest four lines under "Installing…" (editor offer and Settings, Languages). Driven
      live: Svelte on the local test master, TypeScript and Pyright on This PC.
    - `need_gb` measured (peak of the server's process tree, 12 files open, 90 s): TypeScript 0.69 GB → 0.7,
      Svelte 0.61 GB → 0.7, Pyright 0.31 GB on 58 files → 0.4 kept.
- **Preview/inspector on a shown master:** goes through `port_forward`. With a master shown, `localhost` in the
  preview always means the master's localhost, never This PC's:
  - The tunnel uses any free port on This PC, so it never clashes with This PC's own dev servers.
  - The address bar keeps showing `localhost:<port>`, with the machine's name and colour.
  - Public addresses load directly, without a tunnel.
  - Switching back to This PC makes `localhost` mean This PC again.
  - BUILT 2026-11-17 (M1c, not yet driven live):
    - `src/lib/previewTunnel.ts` decides which addresses are loopback (`localhost`, `*.localhost`, `127.x`, `[::1]`;
      80/443 when no port is typed) and swaps in the tunnel's port, keeping site names (`shop.localhost`).
    - `PreviewTab` follows the project's own machine (`project.master`), not only the shown one.
    - History, the address bar and the saved page keep the master's address. The frame, the inspector, "Send this
      page" and "Open in the browser" load the tunnelled one.
    - The bar shows the machine's dot and name. A machine that is not connected says so. A connection that comes
      back asks for a new tunnel (the old ones end with `remote_disconnect`).
    - Known limits: names such as `site.test` are looked up on This PC, not the master. A page that sends the
      browser to its own absolute `localhost:<port>` address leaves the tunnel.
  - Driven live 2026-11-17 on the local test master: tunnel, address bar, relative links, This PC direct, tunnel
    reused after switching back, inspector and page text all pass. Findings p1 (the text attached for the agent
    names the tunnel's address, not the master's) and p2 (machine name cut off in the bar) were fixed and driven
    live the same day.
- **Debugger on a shown master:** js-debug and PHP Debug through the pipe. For Xdebug, the call back to the adapter
  goes through a forwarded port on the master.
  - BUILT 2026-11-17 (M1d), driven live on the local test master the same day (install, js-debug breakpoint and
    continue, PHP Debug with Xdebug, session ends when the machine leaves the screen). On the VPS the same day:
    install, PHP Debug and session end pass; js-debug fails there because js-debug 1.140 needs Node.js 20+ and the
    VPS had Node 18 (findings q1, q2, fixed e564575); with Node 26 there js-debug passes too. How it works:
    - `dap.rs` runs on the lsp `Host`: status, install and the PHP / Xdebug check ask the project's machine.
    - Install: the release is looked up from This PC; the machine downloads and unpacks it into its own data
      folder through two new generic master commands, `host_fetch` (an https archive, tar.gz or zip, swapped in
      only when whole) and `host_read` (a small file below the data folder, for the version note).
    - `Adapters::start_on` runs the adapter through `pipe_start`. PHP Debug (stdio) speaks over the pipe. js-debug
      is started with port 0 on the master, says which port it took, and is reached through `pipes::forward`; child
      sessions open more connections through the same tunnel.
    - Xdebug needs no forward after all: the adapter runs on the master, so PHP there calls it back on the master's
      own localhost.
    - The window: `dap_*` are window-only in remote.ts and take `master`; statuses and the PHP check are kept per
      machine; a session runs on its project's machine; a machine leaving the screen ends its session.
- **Measure** the TypeScript, Svelte and Pyright memory needs (only Intelephense and rust-analyzer are measured) and
  set `need_gb`. DONE 2026-11-17 (see M1b leftovers above).

### M2. The account on the master

- `account_*` moves to the master: the refresh token and the application password go into that machine's secret
  store.
- `qanuvi-master login` (application password).
- The interface's browser sign-in is handed to the shown machine.
- The Account page shows the shown machine's account.
- **Site side (owner's site, separate from this repo):** a "Machine passwords" page on qanuvi.com, and application
  passwords switched on.
- BUILT 2026-11-17 (76a137e); DRIVEN LIVE 2026-11-17 (master side d04b495, window parts in a test build: per-machine Account page, browser sign-in, sign out, cancel). DONE. Stores: This PC keeps `account:<site>` in the credential store; a
  desktop-build master `master-account:<machine id>:<site>`; a server build `account-sign-in.json` (0600, never in
  backups). The browser sign-in is `account_begin` (machine) → `account_wait` (window: browser + loopback) →
  `account_finish` (machine). `qanuvi-master login` / `logout`.

### M3. Each machine owns its server list, and masters connect onward

- Servers become a table in each core's DB. Pairing keys go in that machine's secret store.
- On first start, the interface's localStorage list is imported once into This PC.
- `remote_*` on the master.
- A call to a nested machine carries a path (`["vps1", "vps2"]`). Each master forwards the call and passes stream
  events back.
- Loops are refused: a machine never forwards to a path that already holds its own lasting id.
- The switcher shows the nested dropdown, made visually clear.

Owner decisions (2026-11-17):
- Depth: at most 3 levels below the window (a path holds at most 3 machines). A longer path is refused with a plain
  message, as a loop is.
- Unpairing: when a master drops a server, every machine reaching it through that master loses it at once (open
  connections through it close, the switcher removes it and says why).
- Pairing a server onto a master works both ways: from the window (the window drives it, the master stores the server
  and its key) and from that master's own terminal (`qanuvi-master`).
- Built in three parts, each driven live before the next: M3a server table + one-time import on This PC (no visible
  change); M3b `remote_*` on the master + pairing from a master (desktop-build master and server build); M3c forwarding
  along the path, loop and depth guard, nested dropdown.
- The VPS gets a new server build (with M2) before M3 is driven there (owner yes).
- M3b BUILT + DRIVEN LIVE 2026-11-17 (024d1da): a master answers servers_* and remote_* from its own list and store;
  its Servers screen in the window is that machine's list; `qanuvi-master servers` / `pair` in its terminal. A master
  refuses "local"/"self" as its servers (only the app has them).
- M3c owner decision (2026-11-17): a nested machine behaves exactly as This PC's servers do. Its master's off and
  error servers show too, and picking one has that master connect it; nested rows are always shown (no folding),
  as This PC's list is; preview (port forwarding), language servers and terminals work on a nested machine as on
  a direct one. Depth 3 and the loop guard are driven when the owner provides another machine; until then depth 2
  (window -> local test master -> VPS).
- M3c design:
  - Wire: `remote_call` takes `via` (the rest of the path) and `seen` (lasting ids of the machines passed, the
    window's first). A master refuses a call whose `seen` holds its own id (loop) or whose path would hold more than
    3 machines below the window, then sends `remote_call` on to `master` with the rest. Stream events come back
    hop by hop. A master's hello says `forwards: true`; a path is never sent through one that does not (an older
    master would run the call on the wrong machine).
  - Window: a nested machine's key is its path of server ids joined with "/" ("vps1/vps2"; ids are UUIDs). Every
    store keyed by machine works unchanged; the app's `remote_call`, `port_forward` and language-server pipes take
    the key and go along the path (remote.rs `Routed`, a `pipes::Link`).
  - Each connected master that forwards (and is above the deepest level) has its list followed (`servers_list` +
    `remote_watch`); its servers show indented under it on a guide line. The button shows the route
    ("VPS › build-box"). Its master ending, unpairing or dropping it removes the whole branch at once; a shown one
    lost is held and retried through its master, one taken out says why. The same computer reached two ways shows
    once: the second row says "the same computer as ..." and picking it shows the first.
- M3c BUILT (266d2a5) + DRIVEN LIVE at depth 2 (3fb5878, findings n1-n5 fixed): window -> local test master -> VPS
  (calls, terminal, preview, lost and back). A master that goes away keeps its last known list, so its servers show
  off under it and a shown one comes back through it. Depth 3 and the loop guard wait for the owner's extra machine.
- Owner, 2026-11-17: depth 3 and the loop guard are a later step (M3d in DEVELOPMENT-TASKLIST.md) for when they
  arrange another machine; M3 counts as done for the migration, which goes on with M4.

### M4. This PC runs its own master

- At launch, the app starts or attaches to This PC's master on the app's own data folder.
- There is no data move: the master opens the same `qanuvi.db`.
- The reduced `mark_app_data` mode ends: This PC's master runs its own start-up list, watches, sync and backups.
- Remote access becomes This PC's master accepting relay sessions.
- The extra local hop is measured in M5 (owner, 2026-11-18): in M4 the window still calls its own core directly.

Owner decisions (2026-11-18):
- Built in three parts, each driven live before the next:
  - M4a: the app always starts This PC's master at launch (or attaches to the running one), remote access on or not.
    The master opens the same `qanuvi.db`. The window still calls its own core (that is M5).
  - M4b: the background jobs move to the master and the reduced `mark_app_data` mode ends. The master runs the
    start-up list, Claude Code auto-update, the OmniRoute watch, memory sync and backups; the app stops running
    them, so nothing runs twice. Remote access is the master taking relay sessions.
  - M4c: lifetime. A tray icon whenever the master runs (never hidden), with "Open Qanuvi" and "Quit Qanuvi" (Quit
    stops the master). Before an update installs, the app stops the master (Windows locks its file).
- Closing the last window: the master keeps running with its tray icon (R4). Until M5 only the background jobs go
  on; agents, plan runs and terminals still live in the app and end with it.
- A setting, off by default: "Start Qanuvi in the background when I sign in".
- M4a BUILT + DRIVEN LIVE (246e6a5). M4b BUILT + DRIVEN LIVE (2026-11-18): This PC's master runs the start-up list,
  Claude Code update, OmniRoute watch, memory disk sync and daily backups (the app's backups folder, with the window
  storage); the app runs none of them. The window's start-up commands go to the master (`this_pc_call`), and the
  app follows the master's Inbox (`follow_this_pc_inbox`) so its notices reach every window and notification.
  Until M5 the app keeps its running sessions and runs, a booked restore and This PC's server connections.
- M4c BUILT + DRIVEN LIVE (2026-11-18), M4 DONE: This PC's master shows a tray icon while it runs ("Open Qanuvi",
  "Quit Qanuvi"; a click opens), stays when the last window closes, and starts at sign-in when "Start Qanuvi in the
  background when I sign in" is on (off by default; `qanuvi --background` starts the master with no window). The
  window stops the master before an update installs. The tray lives in the `qanuvi-master` program only.

### M5. The window becomes a viewer

- Every call goes over the protocol to its machine. This PC is just a machine id.
- The direct Tauri calls in stores move to the router.
- Desktop `app.emit` events are replaced by the master streams (`inbox_watch`, `plan_run_watch`).
- This PC's coding tools use the same pipes as every other machine.
- Window state (shown machine, layout, tabs, panel, machine windows) stays in the window.

Owner decisions (2026-11-18):
- No adapting to the old core: the owner is the only user, so there is no mixed state to support. M6 folds into
  M5a (the second core is deleted in the same pass), and the "two processes on one DB" rule ends with it.
- Built in three parts, each driven live on This PC (every page) before the next:
  - M5a, the cut-over: all of This PC's calls and streams (plain calls, terminals, agent sessions, plan runs, the
    Inbox) go to its master over one link, as for any paired master. The second core goes (M6 below). The hop is
    measured on whole actions as the user feels them (open a project, expand the tree, search, open a file, git
    status), direct against through the master, and reported to the owner. Notices:
    - The last window closing while work runs says what keeps going ("2 agents and 1 plan run keep going on this
      computer. Quit Qanuvi from the tray icon to stop them."); with nothing running it closes quietly.
    - With no window following it, the master raises the OS notifications itself (a blocked step's question or a
      budget stop never waits unseen).
    - Quit and installing an update say first what they will stop; afterwards the run says why it stopped
      ("stopped by update"), with Resume.
  - M5b, stops and restarts:
    - `qanuvi-master` becomes a small watcher (tray icon, restarts) plus the core that does the work. A crashed core
      is restarted with a short wait, and the watcher keeps why (exit code, last log lines, what it was doing).
    - Crash limit by count, not by time: on the 3rd crash the watcher stops restarting and the brain investigates.
      It reads the crash notes with a cheap local model first and checks known causes (port taken, locked database,
      a start-up item that crashes it, out of memory). Safe fixes run on their own (free the port, clear a stale
      lock, start once without the crashing item) and the owner is told what was done. Anything touching data,
      settings or Qanuvi's code, or anything unexplained, goes to the owner as a notification with the report; no
      change until a yes. The count resets after a clean Quit or a long clean run.
    - A run never stops silently: "stopped when the computer shut down" / "stopped when Qanuvi's background part
      crashed", with Resume.
    - With "Start Qanuvi when Windows starts" on, cut-off runs resume on their own after the restart (and say so).
      With it off, the next launch shows the stop with Resume and offers the setting, each time it would have
      helped, with "Don't ask again".
    - The setting is renamed from "...when I sign in" to "Start Qanuvi when Windows starts", so it is never read as
      the Qanuvi account.
    - Qanuvi cleans up after itself (owner, 2026-11-18; measured in %TEMP%: qanuvi-browser-* test browser profiles
      0.7 GB, about 25 Qanuvi-0.1.x-updater-* folders 0.25 GB, one per update and never removed):
      - A headless browser's profile (`browser.rs` `Browser::launch`, `qanuvi-browser-<pid>-<uuid>`) is removed
        when that browser ends, and when the master exits.
      - At start the master removes leftover `qanuvi-browser-<pid>-*` profiles whose pid is no longer running (a
        crash or power cut skips the exit cleanup), never one of a running process.
      - After an update has installed, the older `Qanuvi-*-updater-*` folders are removed.
  - M5c, the rest: This PC's coding tools (language servers, debuggers, inspector) through the same pipes as every
    machine; a booked restore and This PC's server connections on its master; i1 (unsaved text kept as drafts,
    shown as "Draft, not saved to disk"); j1 (a terminal whose shell ended says so); "earlier output trimmed" after a
    reconnect; a restarting language server shows "checking…" instead of an empty list of problems.

- M5a BUILT (2026-11-18: b633c95 cut-over with the second core deleted, 4f9f15f notices + r1 loop-guard fix); live
  drive pending. Hop measured: the stdio bridge adds 0.1 to 2.5 ms per call, so no direct TCP link is needed.
- M5a DRIVEN LIVE (2026-11-18): s1 one master per data folder (master.lock), s2 it/they wording, s3 an open note
  seen again is not told again, s4 no "keeps working" notice on tray Quit, s5 the detached service inherits no
  handles (Windows); details in DEVELOPMENT-TASKLIST.md.
- M5b BUILT (2026-11-18): `serve` is the watcher (tray, restarts, master.log), `core` does the work; crash limit 3
  with known causes, a local model's reading and one safe fix (a start without the start-up list); runs say
  shutdown / crash / ended with Resume and resume on their own after a restart when Qanuvi starts with Windows (else
  the Run panel offers the setting); the setting is "Start Qanuvi when Windows starts"; browser profiles and updater
  folders are cleaned up. Details in DEVELOPMENT-TASKLIST.md.
- M5c PART 1 BUILT (2026-11-18): This PC's language servers and debuggers run on its master through the program
  pipe, as every machine's (the app starts none itself); j1, "earlier output trimmed", "Checking…" for a
  restarting language server, and i1 (drafts kept when a window goes, with an Inbox note). Owner (2026-11-18): This PC's
  master holds This PC's server connections and the window reaches them through it (M5c part 2, to build); the
  booked restore stays in the app (done). Details in
  DEVELOPMENT-TASKLIST.md.
- M5c PART 2 BUILT (2026-11-18): This PC's master holds This PC's server connections (connects them at start,
  window or not); the window asks it to connect, follows its watch, and its calls go through it. Switcher keys are
  unchanged (no move of open state); "local" and "self" stay the window's. Also: one connect at a time per machine,
  and what a window opened through a master ends when that window goes. Live drive pending; details in
  DEVELOPMENT-TASKLIST.md.

### M6. Delete the second core (folded into M5a, owner 2026-11-18)

- From `desktop.rs`, remove the core wrappers, plus `desktop_runs.rs`, the Inbox copy, the loops and the shutdown
  copies.
- The parity test then checks that the app answers only the interface list.

### M7. A master in a terminal

- `qanuvi-master status`, `login`, `projects`, `inbox`, `plan run <file>`, `servers add/list`.
- These are thin clients to the local service over the same protocol.
- A full terminal screen goes later (IDEAS-UPCOMING).
- AGREED (owner, 2026-11-18): M7a `status` + `projects list/add/remove`; M7b `inbox` (list, show, answer); M7c
  `plan run <file>` (follows the run's lines) + `plan list/stop`. Ctrl+C on a followed run only detaches the
  terminal; the run is the master's and goes on until `plan stop`.
- M7a BUILT + DRIVEN LIVE (2026-11-18) on a Windows scratch master and the VPS.
- M7b BUILT + DRIVEN LIVE (2026-11-18) on a Windows scratch master and the VPS.
- M7c BUILT + DRIVEN LIVE (2026-11-18) on a Windows scratch master (VPS still to drive). Also `plan follow`, so a
  terminal can follow a run again after Ctrl+C. A terminal run takes the setup of the last run a window started on
  that master (run-starts/); with none it says to start one run from a window first. M7 DONE on Windows.

### M8. Masters update themselves (agreed 2026-11-17)

The rule: the master does everything the app does today, updates included.

- **Today (0.1.23):** the app screen is This PC's core, so it does the updating itself.
- **After the migration:** updating is the master's job only. The screen does no updating of its own. It is a
  viewing container with settings: it shows a master's update state, offers the button, and switches that master's
  update settings, as it switches other settings today.

- **Checks:** each master looks for a newer version on the same signed update channel, as the app does today
  (hourly, with a signature check; a wrong signature is refused). It reads the channel through the same
  `latest.json`, which then lists master builds too (Windows, Linux x86_64 server build, macOS) next to the app
  installer. `publish-update.mjs` builds and signs those as well.
- **In a terminal:**
  - `qanuvi-master status` and the start of every terminal command print "Update available: 0.x.y" when there is
    one.
  - `qanuvi-master update` downloads, checks the signature, swaps the program and restarts the service. The
    service keeps the old copy (.prev) to go back to.
  - In a terminal the master only tells; it does not update by itself unless its auto-update setting is on.
- **In the interface, for the shown master,** the same look as for This PC today:
  - a slim "Update to x" button in the status bar with progress, then "Restart to update";
  - the master does the work, and the screen only asks it and shows its progress.
- **Auto-update** ("Download updates by themselves") is a setting stored on each master:
  - It is switched from the screen, as today.
  - It is off by default for a master set up from a terminal.
  - When on, the master updates by itself, still waiting for running work.
- **Running work:** the master uses its existing `master_busy`. While agents or plan runs are working, the restart
  waits and says so: on screen, in the terminal, and as an Inbox note. It never cuts a run off.
- **This PC:** after M4, This PC's master updates the whole install, both the master and the screen app, with one
  installer and one button. The screen app has no updater of its own any more.
- **Publishing:** an update still goes out only with the owner's yes.

### M9. Release

- Full verify, plus a long everyday check on scratch projects: This PC, the local test master, the VPS, a nested
  master, and coding tools on a shown master.
- Old installs: the server list import and existing data.
- Publish only with the owner's yes.

### M10. Account gate and device discovery (owner, 2026-11-18; after the system is built)

- Signed out of the Qanuvi account, Qanuvi is a plain code editor: This PC's master runs only while a window is
  open; no models, agents or plan runs, sync, memory, account settings, background work, tray staying, or "start
  when Windows starts" offer.
- Signed in, everything is unlocked. The gate sits in one place on the master, not spread over the screens.
- No master (a VPS included) can be added to any app unless both sides are signed in to the same account.
- Discovery: every device signed in to the same account shows in the app's machine list as "not added yet". One
  click adds it: the two masters set up the pairing themselves (make and swap the keys, reusing today's pairing keys
  and relay; nothing typed), and it joins This PC's list.
- Safety: only the account's own devices are shown or can be added; the added machine says "added by <machine>"
  with Remove; signing a device out takes it off every list.

## Nothing in Qanuvi's plan changes (owner, 2026-11-17)

Everything built and planned for Qanuvi stays as it is. Only the layer design changes: the screen becomes a
container, and the work moves into a master. After the migration the screen looks the same, a complete screen for
whichever machine is shown, This PC or any master.

Every open item of the old plan is kept and lands here:

| Old item | Where it lands |
|---|---|
| R3 queue (migration 10) | On the master, as already decided: each machine's queue in its own DB, scheduler in its core |
| R4 tray mode | This PC's master's tray icon (M4): visible whenever it runs (never hidden), opens the screen, "Quit Qanuvi" stops the master. Closing the screen only closes the container; runs, the queue and agents go on. No separate "tray mode" is needed |
| R5 drive live | In M9's long check: a scratch plan, close the screen, reopen, progress intact; then the VPS |
| R6 leftover processes | On the master: every child it starts in a kill-on-close job / process group, with a start-up check for leftovers. The owner rule stays: never end what the master still handles |
| i1 unsaved files on window close | M5: a closing viewer keeps unsaved text as drafts on the shown machine, and the Inbox says so |
| i2 browser helper left after close | With R6: agent processes and `--mcp-browser` in the master's job |
| 24z3 Part F (window memory with several masters connected) | M9: measure with several masters shown in turn and nested ones |
| j1 blank restored terminal | M5: a terminal whose shell ended says so |
| n1 a blocked step's question says too little (owner, 2026-11-17) | First item after M9, on the master. See n1 in DEVELOPMENT-TASKLIST.md |
| Everything else in DEVELOPMENT-TASKLIST.md | Unchanged, built on the master after the migration |

## Decided

- **This PC's master after the screen closes:** it keeps running with its tray icon visible, until the user quits
  from the icon (this is R4 in the new design).
- **Releases:** M1 alone, then M2+M3, then M4-M6 together, then M7-M9, then M10. Each only with the owner's yes.

## Risks

- **M4 and M5 change This PC's base.** Live checks must cover every page on This PC.
- **Two processes on one DB** between M4 and M6: the app core must not write core tables.
- **Speed** of the local hop for This PC (measure in M4).
- **Streams on reconnect:** the Inbox and plan runs reload; language servers and debuggers restart.
