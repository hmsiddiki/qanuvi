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
  - Not yet: live install output (the install still answers at its end, as on This PC); `need_gb` for TypeScript,
    Svelte and Pyright is not measured yet. Driven live through the app window on the local test master
    2026-11-17 (VPS not yet).
- **Preview/inspector on a shown master:** goes through `port_forward`. With a master shown, `localhost` in the
  preview always means the master's localhost, never This PC's:
  - The tunnel uses any free port on This PC, so it never clashes with This PC's own dev servers.
  - The address bar keeps showing `localhost:<port>`, with the machine's name and colour.
  - Public addresses load directly, without a tunnel.
  - Switching back to This PC makes `localhost` mean This PC again.
- **Debugger on a shown master:** js-debug and PHP Debug through the pipe. For Xdebug, the call back to the adapter
  goes through a forwarded port on the master.
- **Measure** the TypeScript, Svelte and Pyright memory needs (only Intelephense and rust-analyzer are measured) and
  set `need_gb`.

### M2. The account on the master

- `account_*` moves to the master: the refresh token and the application password go into that machine's secret
  store.
- `qanuvi-master login` (application password).
- The interface's browser sign-in is handed to the shown machine.
- The Account page shows the shown machine's account.
- **Site side (owner's site, separate from this repo):** a "Machine passwords" page on qanuvi.com, and application
  passwords switched on.

### M3. Each machine owns its server list, and masters connect onward

- Servers become a table in each core's DB. Pairing keys go in that machine's secret store.
- On first start, the interface's localStorage list is imported once into This PC.
- `remote_*` on the master.
- A call to a nested machine carries a path (`["vps1", "vps2"]`). Each master forwards the call and passes stream
  events back.
- Loops are refused: a machine never forwards to a path that already holds its own lasting id.
- The switcher shows the nested dropdown, made visually clear.

### M4. This PC runs its own master

- At launch, the app starts or attaches to This PC's master on the app's own data folder.
- There is no data move: the master opens the same `qanuvi.db`.
- The reduced `mark_app_data` mode ends: This PC's master runs its own start-up list, watches, sync and backups.
- Remote access becomes This PC's master accepting relay sessions.
- Measure the extra local hop: typing, file reads and the terminal must still feel instant.

### M5. The window becomes a viewer

- Every call goes over the protocol to its machine. This PC is just a machine id.
- The direct Tauri calls in stores move to the router.
- Desktop `app.emit` events are replaced by the master streams (`inbox_watch`, `plan_run_watch`).
- This PC's coding tools use the same pipes as every other machine.
- Window state (shown machine, layout, tabs, panel, machine windows) stays in the window.

### M6. Delete the second core

- From `desktop.rs`, remove the core wrappers, plus `desktop_runs.rs`, the Inbox copy, the loops and the shutdown
  copies.
- The parity test then checks that the app answers only the interface list.

### M7. A master in a terminal

- `qanuvi-master status`, `login`, `projects`, `inbox`, `plan run <file>`, `servers add/list`.
- These are thin clients to the local service over the same protocol.
- A full terminal screen goes later (IDEAS-UPCOMING).

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
- **Releases:** M1 alone, then M2+M3, then M4-M6 together, then M7-M9. Each only with the owner's yes.

## Risks

- **M4 and M5 change This PC's base.** Live checks must cover every page on This PC.
- **Two processes on one DB** between M4 and M6: the app core must not write core tables.
- **Speed** of the local hop for This PC (measure in M4).
- **Streams on reconnect:** the Inbox and plan runs reload; language servers and debuggers restart.
