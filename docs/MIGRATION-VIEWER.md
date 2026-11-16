# Migration plan: the app is a viewer of masters

Status: DRAFT for the owner's review (2026-11-16). No code until the owner agrees.
Going back: branch `old-infrastructure` / tag `v0.1.23-old-infrastructure` (see OLD-INFRASTRUCTURE.md there), data
backup at `E:\Backups\qanuvi\old-infrastructure-0.1.23`.

## The goal (owner decision 2026-11-16)

- The master is Qanuvi: projects, files, git, terminals, agents, plans, Inbox, memory, settings and its own server list.
  It works fully with no display.
- The app is only the visual container: editors, chat, voice, layout, windows. It shows one master at a time.
- This PC runs its own master, and the app shows it by default. A VPS runs only a master.
- A master can connect to other masters. The app shows them nested under that master in the switcher.
- Nothing on screen is shared with This PC. Every machine shows the same screen with its own data.

## Where we are (measured at 0.1.23)

- **Two cores, two dispatchers.**
  - The app's built-in core: `desktop.rs`, 212 Tauri commands, one typed wrapper each.
  - The master: `master.rs`, `Master::dispatch`, 175 commands as string arms; `plan_run_*` and `inbox_*` sit in their
    own files.
  - Both call the same core modules but duplicate the wiring (Inbox, plan runs, background loops, shutdown).
  - No test checks that they offer the same commands.
- **The gap is 49 commands only the app answers.**
  - Inbox and plan runs are already on the master, apart from `inbox_import`.
  - Window-only by nature (16): `notify_*` (4), `account_*` (4), `app_info`, `app_restart`, `startup_path`,
    `term_kill_window`, `lsp_stop_window`, `dap_stop_window`, `inbox_import`, `plan_data_import`.
  - Connecting to other machines (6): `remote_*`. These are window-only today; in the target they belong to every core.
  - App install (4): `data_location_*` (3) and `data_test_build`. A master's folder is fixed by `--data`.
  - Core, could move as is (10): `inspector_*` (5), `browser_agent_shot`, `core_decide`, `knowledge_fetch`,
    `attachment_page`, `attachment_image`.
  - Core with an event stream (11): `lsp_*` (5) and `dap_*` (6), apart from the `*_stop_window` ones. They need the
    `onEvent` stream wiring that `term_spawn` already uses, and cleanup when the connection ends.
  - Split (1): `attachment_file` reads a file picked on this screen. It stays in the window, which sends the bytes.
- **The window routes calls.**
  - `src/lib/remote.ts` sends each call to This PC (Tauri invoke) or to a master (`remote_call`).
  - The choice is based on `WINDOW_ONLY`, then the owner of the id, then the shown machine.
  - Several stores call Tauri directly and skip the router.
- **The server list belongs to the window.**
  - It lives in `settings.masters` in localStorage, and `settings.rs` refuses to store it in a database.
  - Pairing keys are in This PC's keyring.
  - A master cannot connect onward, and no master forwards calls.
- **This PC is not a master.**
  - A `qanuvi-master` runs on the app's own `qanuvi.db` only for remote access (`target: "self"`).
  - That master runs in a reduced mode (`mark_app_data`): no start-up list, watches, sync or backups.

## The target

```
App window ──(one protocol)──> This PC's master ──> its own DB, terminals, agents, plans
                     │                          └──> masters it connects to ──> their masters (nested)
                     └──> any other master the app was given directly
```

- **One command set, one dispatcher (the master's).** The app's Rust side keeps only window things: notifications,
  account sign-in, app info/restart/update, the data folder of the install, window-to-window events, and the client
  side of connections.
- **Every call goes over the same protocol, This PC included.**
  - The router becomes: the owner of the id, else the shown machine.
  - `WINDOW_ONLY` shrinks to the true window list.
- **The machine owns its server list and keys.** The switcher shows This PC's list. A master that has its own list
  shows a nested dropdown.
- **Calls to a nested machine are forwarded.** The call carries a path (`["vps1", "vps2"]`). Each master passes the
  call to the next one and passes stream events back.

## Phases

Each phase ends with verify green, a live check on scratch projects, and the owner's review. A release comes only
with the owner's yes.

### M0. Guard rails (small)

- A parity test lists every desktop command and fails if the master does not answer it.
- The 16 window-only names are written in one shared list that the test allows.
- Today the test would fail with 33 gaps. Each phase below removes some, until none are left.

### M1. Close the core gap on the master

- **Move as is:** `inspector_*`, `browser_agent_shot`, `core_decide` (keys via `model_keys::get`), `knowledge_fetch`,
  `attachment_page`, `attachment_image` (the master's own stored images).
- **LSP and DAP on the master:**
  - Events go out through connection streams.
  - Servers end when their connection ends, the same as terminals do today.
  - A language server runs on the machine that has the files. This also ends the "LSP refuses master projects" gap.
- **Connections (`remote_*`) on the master:**
  - Add `Remotes` to the master.
  - `remote_connect` / `remote_call` / `remote_disconnect` / `remote_list`, with events passed through.
- **Data-folder moving** stays an app-install feature. A master reports `fixed: true` as today.

### M2. Each machine owns its server list

- Servers are a table in each core's DB, replacing `settings.masters` in localStorage.
- Pairing keys are kept in that machine's secret store.
- One-time import on first start: the app's localStorage list goes into This PC's core.
- Name, colour and lasting id rules (24z3) move with it unchanged.

### M3. This PC runs its own master

- At launch, the app starts This PC's master, or attaches to one already running. It uses the existing
  `serve` + `--stdio` bridge on the app's own data folder.
- **No data move:** the master opens the same `qanuvi.db`.
- The app's built-in core stops opening it for core work.
- The reduced `mark_app_data` mode goes away: This PC's master runs the start-up list, watches, sync and backups
  itself. They are removed from `desktop.rs`.
- Remote access becomes "This PC's master accepts relay sessions". It needs no second process.
- The installer already ships `qanuvi-master.exe` next to the app. Check this in the release build.

### M4. The window becomes a viewer

- `remote.ts`: every call goes over the protocol to its machine. This PC is a machine id like any other.
- The direct Tauri calls in stores (`inbox`, `planData`, `settings`, `masters`, `remoteAccess`, ...) move to the
  router.
- **Events:** desktop `app.emit` events (`qanuvi-inbox`, `qanuvi-plan-data`, `qanuvi-run*`) are replaced by the
  master streams already used for other machines (`inbox_watch`, `plan_run_watch`). `notify_item` stays in the
  window, fed by those streams.
- **Switcher:**
  - Machines come from the shown core's server list.
  - A master that has its own list shows a nested dropdown, made visually clear.
  - Calls use a machine path.
- **Window state stays in the window:** shown machine, layout, tabs, panel, split, machine windows.

### M5. Delete the second core

- From `desktop.rs`, remove every core command wrapper, plus `desktop_runs.rs`, the Inbox copy, background loops and
  shutdown copies.
- What is left: the window commands and the connection client.
- The parity test now checks that the app answers only the window list.

### M6. A master in a terminal

The owner said a master can be driven fully from a terminal. Minimal commands:

- `qanuvi-master status`
- `projects`
- `inbox`
- `plan run <file>`
- `servers add/list`

These are thin clients over the same protocol to the local service. The full scope is the owner's choice, see the
decisions below.

### M7. Release

- Full verify, plus a long everyday check on scratch projects: This PC, the local test master, the VPS and one nested
  master.
- Check old installs: the server list import and existing data.
- Publish only with the owner's yes.

## Decisions for the owner

1. **This PC's master after the app closes:** recommend it keeps running while work is in progress (plans, agents)
   and stops when idle, with a "Stop This PC's master" item. This overlaps R4 (tray).
2. **M6 scope:** recommend the minimal commands above in this plan. A full terminal UI goes later
   (IDEAS-UPCOMING).
3. **Nested depth:** recommend no limit in the protocol, with loops refused (a machine never forwards to a path that
   already contains its own lasting id).
4. **Account sign-in** (Qlopy OAuth) stays per device in the window. A master uses relay pairing keys, not the
   account.
5. **Order:** recommend M0 → M1 → M2 → M3 → M4 → M5, then M6, then M7. M1 and M2 can be released on their own, since
   they only add to the master. M3–M5 change the app's base and go out together.

## Risks

- **M3 and M4 change everything at once for This PC.** Live checks must cover every page on This PC, not only on
  masters.
- **Two processes on one DB** during M3: until M5, the app core must not write core tables. Mitigate with a
  read-only open in the app.
- **Speed:** every This PC call gains a local socket hop. Measure in M3, since editor typing, file reads and the
  terminal must still feel instant.
- **Event loss on reconnect:** streams need a resend or reload on reconnect. Inbox and plan runs already reload; LSP
  and DAP restart.
