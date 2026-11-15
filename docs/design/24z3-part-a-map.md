# 24z3 Part A, step 1: map of the window's calls (read only, 2026-11-15)

How the window reaches a core today, and what has to change so that every screen calls "the shown machine".
Nothing here is built; this is the map the owner reviews before Part A's code.

## How calls travel today

- `src/lib/api.ts` (most of the interface) calls `routed()` in `src/lib/remote.ts`. `routed()` looks at the call's
  `projectId`, `sessionId` or terminal `id`. If a master made that id, the call goes to the master over `remote_call`;
  otherwise it goes to This PC.
- So routing follows **ids**, not a shown machine. A call with no id always goes to This PC, even when the owner is
  looking at a master's project. Today that affects:
  - `projects_list`, `projects_add` (adding a folder always adds it on This PC);
  - `term_shells` (the shell list is This PC's);
  - `plan_run_active`, `agent_usage_since` (This PC's runs and costs).
- Five stores pick a master themselves, with `callMaster()`:
  - `stores/runner.svelte.ts`: `plan_run_watch` / `plan_run_active` on the master that owns the project;
  - `plan/planData.svelte.ts`: `plan_data_list` / `plan_data_set` on the project's master;
  - `stores/inbox.svelte.ts`: `inbox_list` / `inbox_watch` / answers on every followed master, plus This PC
    (this is already what Part C2 wants: keep it, add the badges and the filter);
  - `stores/remoteAccess.svelte.ts`: `relay_*` on This PC's own master (remote access settings);
  - `stores/masters.svelte.ts`: pairs and connects masters, then puts each master's `projects_list` into the same
    project list (`projects.addRemote`). **This is where the sidebar's mixing comes from.**
- 18 more files call `invoke` directly (This PC only): Settings panels, setup, account, models, updates, notifications.

## Commands a master answers today (100)

projects_*, files_*, git_*, index_*, snapshots_*, term_spawn/write/resize/kill/shells, agent sessions (start, stop,
respond, interrupt, rename, delete, link, info, events, times, saved, grants, handover, file history, usage),
browser_agent_view, plan_run_* (with plan_run_watch), plan_private*, plan_data_list/set, runs_save/list,
inbox_* (with inbox_watch), relay_*, checks_run, hello.

## Commands the window calls that a master does NOT answer

Per Part A's rule, while one of these is missing and a master is shown, the screen says "not on VPS yet" and never
uses This PC instead.

### 1. Project work (the files are on the machine, so these must run there)

| Group | Commands | Used by |
| --- | --- | --- |
| Language servers | lsp_start, lsp_send, lsp_stop, lsp_stop_window, lsp_statuses, lsp_install | editor completion, hover, formatting |
| Debugging | dap_start, dap_connect, dap_send, dap_stop, dap_stop_window, dap_statuses, dap_install, dap_php | Run and debug |
| Project memory | memory_list, memory_save, memory_search, memory_slice, memory_delete | agent memory, notes |
| Agent setup | agent_config_list, agent_config_read, agent_config_write, agent_configure, agent_clean_up | agent pane, model picker |
| Other | core_decide, attachment_page, browser_agent_shot, startup_path, term_kill_window | agent pane, terminals |

### 2. Settings (Part D: the whole Settings page is the shown machine's)

| Group | Commands |
| --- | --- |
| The settings themselves | **none**: Settings live in the window's localStorage (`qanuvi.settings.v1`); a master has no settings store |
| Keys | secrets_names, secrets_set, secrets_remove (a master reads `QANUVI_KEY_...` only) |
| Models | models_list, model_key_has, model_key_set, model_key_check, model_probe, local_server_start, local_server_status, ollama_pull |
| OmniRoute | omniroute_connect, omniroute_catalog, omniroute_connections, omniroute_add_key, omniroute_make_combo, omniroute_remove, omniroute_sign_in, omniroute_test |
| Backups | backups_info, backups_now, backups_set, backups_restore, backups_cancel_restore, backups_remove |
| Data folder | data_location_info, data_location_move, data_location_cancel, data_location_remove_old |
| Startup / Headroom | startup_status, startup_start, startup_stop, startup_restart, startup_headroom, claude_settings_base_url |
| Claude Code updates | claude_versions, claude_update |
| Notifications | notify_settings |
| Usage | qanuvi_usage |
| Memory disk | memory_disk_sync, memory_disk_versions, memory_disk_restore, memory_disk_passphrase_set/_saved/_forget |
| Setup | machine_info, machine_room, setup_tools, setup_install |

### 3. Stay with the window (never routed to a machine)

app_info, app_restart, data_test_build, remote_* (the switcher's list of masters), notify_item / notify_take_start /
notify_test (the desktop banner shows on this screen), attachment_file / attachment_image (a file picked on this
screen for a message), inspector_* (the preview pane in this window), knowledge_fetch (Qanuvi's signed knowledge
file), inbox_import / plan_data_import (one-time moves from old localStorage).

## Window state kept in localStorage (Part B/D: becomes the shown machine's)

| Key | What |
| --- | --- |
| qanuvi.settings.v1 | the whole Settings page |
| qanuvi.layout.v1, qanuvi.panel.v2, qanuvi.editorSplit, qanuvi.diff.layout | layout (Part D: kept on the machine) |
| qanuvi.currentProject, qanuvi.workspace.<project> | open project, open tabs per project |
| qanuvi.plan.<id>, qanuvi.search.v1 | Plan view and search panel state |
| qanuvi.breakpoints.v1, qanuvi.debug.pick.v1, qanuvi.debug.watch.v1 | debugging |
| qanuvi.guides.v1, qanuvi.knowledge.*, qanuvi.omni.muted, qanuvi.memoryDisk.last | guides, knowledge, notices |
| qanuvi.inbox.v1, qanuvi.picks./plannotes./planstate./run. | old copies, already moved into the DB |

## Questions for the owner

1. **Account sign-in and the memory disk.** The design says the brain belongs to the account. Proposal: the Qlopy
   sign-in stays with the window (it is you, on every machine you show); each machine runs its own memory disk sync,
   set from its Settings page (Part D).
2. **Language servers and the debugger on a master.** Proposal: they run on the master, where the files are, but as
   a later part of Part A. Until then a master's editor works without completion and debugging, and says so.

## Proposed order for Part A's code (each in its own conversation)

1. **A1, the shown machine:** `remote.ts` gets a shown machine (This PC by default). A call with no id goes to it, not
   to This PC; an id owned by another machine is refused, never rerouted. One list of missing commands, so a missing
   one shows "not on VPS yet". Inbox keeps calling every machine (Part C2).
2. **A2, the project list:** the sidebar asks only the shown machine for `projects_list`, so the master's projects
   stop mixing into This PC's list (`projects.addRemote` goes).
3. **A3, settings on the machine:** a settings store in each machine's DB (`settings_get` / `settings_set`), keys
   (`secrets_*`) on the master, then the Settings groups above in batches.
4. **A4, project work on a master:** project memory and agent setup first, then language servers and debugging.
