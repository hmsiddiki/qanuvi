# Old infrastructure (backup of 0.1.23)

This branch, `old-infrastructure`, and the tag `v0.1.23-old-infrastructure` keep Qanuvi exactly as it was published
as 0.1.23 on 2026-11-16, before the migration plan ("the app is only a viewer of masters") started on `main`.

## How it works here

- The window has its own built-in core (desktop.rs, 199 commands). Masters (`qanuvi-master`) answer 152 of them.
- 57 commands are desktop-only (inbox_*, remote_*, lsp_*, dap_*, notify_*, account_*, inspector_*, attachment_*,
  data_location_*, ...). Screen parts were moved to follow the shown machine one at a time (24z3 Parts A-E, steps
  1-4 in DEVELOPMENT-TASKLIST.md).

## Going on from here without the migration

To resume this line: check out this branch (`git switch old-infrastructure`), say "skip the migration plan and go on
with the old order", and continue from the open items the migration replaced:

1. i1: a closing window or app keeps unsaved files as drafts on their machine (B2 park path in onCloseRequested;
   the Inbox says so).
2. i2: agent processes in a job object, so `--mcp-browser` ends with them.
3. 24z3 step 5 Part F.
4. R3 (queue, migration 10), R4 tray mode, R5 live, R6 cleanup of leftover sessions; then a full check and a release.
5. Finding j1: a restored terminal tab whose shell ended with its master stays blank; it should say the shell ended.

Data from that time: see the backup folder named in the tasklist entry for 0.1.23 (database snapshot of the
installed app). The 0.1.23 installer stays on the dev update channel.
