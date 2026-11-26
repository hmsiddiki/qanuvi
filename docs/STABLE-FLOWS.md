# ST1: everyday flows on Windows and Linux

One row per flow a normal user meets. Each one is driven for real on the release build:
- Windows: a test window on This PC, in one block of time the owner gives off the PC;
- Linux: a test window on the laptop over SSH (drive.py);
- scratch projects and test data only.

Results:
- `pass`
- `FAIL <id>`, with the fix in ST2
- `-`, not driven yet

A row is green only when both systems pass.

| # | Flow | What passes | Windows | Linux |
|---|------|-------------|---------|-------|
| 1 | First start, empty data | window opens, setup wizard offered, no error lines | pass | pass |
| 2 | Setup wizard, free AI | OmniRoute / OpenRouter / Ollama steps say what they do; a free model answers | - | pass (OpenRouter) |
| 3 | Open a project, file tree, editor | open, edit, save, undo; big file opens fast | pass | pass (s2, s3) |
| 4 | Search and go to symbol | Ctrl+P, Ctrl+T, find in files | pass | pass after f1 |
| 5 | Git panel | changes, diff, commit on a scratch repo; no-git folder works | pass | pass after f2-f4 |
| 6 | Terminal | open, type, colours, resize, paste, close; survives switching projects | pass after w1 (s19) | pass after f5 (s4) |
| 7 | Chat with the paid agent | answer streams, tool rows settle, cost shown | pass | pass |
| 8 | Chat with a free model (Qanuvi agent) | answer, a tool call, a hand-over when a model rests | - | pass (hand-over not driven) |
| 9 | Approve and revert an edit | Manual mode asks; Revert puts the file back | pass | pass |
| 10 | Message while the agent works | taken as a chat, not a restart | pass | pass after f6 |
| 11 | Memory page | add, Try it, suggestion Keep/Dismiss | pass | pass |
| 12 | Write a plan with the planner | plan file made, steps readable | pass | pass after f8 |
| 13 | Check before the run | card lists needs plainly; answers kept | pass | pass (s8) |
| 14 | Plan run, two steps, real agent | both done, honest end line, report | pass | pass |
| 15 | Held step and the Inbox | question in plain words; answer from Inbox goes on | pass | pass |
| 16 | List to look at | half-orange step; typed answer becomes a step; All fine clears it | - | pass |
| 17 | Stop, pause, resume a run | each says what happened; nothing lost | pause + resume pass; stop not caught (s19) | pass after f9-f11 (s9) |
| 18 | Queue | add, reorder, next task starts after a run ends | - | later: owner moved it to the Tasks idea (docs/IDEAS-UPCOMING.md) |
| 19 | Close to tray and open again | runs go on; window comes back to the same place | pass | pass after f12 |
| 20 | Quit while a run goes on | asks; "go on next time" parks it; next start resumes | - | pass; "go on next time" built + driven 2026-11-25 (s10) |
| 21 | Updates | Settings shows the right line; update from a test channel restarts on the new version | - | pass (s12) |
| 22 | Pair a machine | code pairing over the relay; machine in the switcher | - | pass (s13) |
| 23 | Switch machines | the other machine's projects, chat, terminal; back again | - | pass (s13) |
| 24 | Direct link and fallback | "connected · direct" at home; relay on a hotspot; back by itself | - | pass (s15) |
| 25 | Network drop and back | reconnecting line, then connected; no stale lines | - | pass after c1, c2 (s16) |
| 26 | Sleep and wake | links, runs and terminals come back | - | pass; runs not checked (s17) |
| 27 | Preview and live watch | preview address opens; live mirror on request | preview pass; live mirror not driven | preview pass; live mirror not driven |
| 28 | Backups and data folder | daily backup made; restore of one table on scratch data | backup now pass; restore not driven | pass after ba1, ba2 (s14) |
| 29 | Settings | every page opens, scrolls, saves; Save always reachable | pass | pass |
| 30 | Status bar | account, branch, servers, memory sync, agents, cost: each opens its page | pass | pass (s11) |
| 31 | Fresh install as a new user | installer / tarball on a clean user; first start works | - | pass after n31a (s18) |
| 32 | Go back one version | previous build installs over; data intact | - | pass (s12) |

ST3, the 24 h run, has its own log. It covers memory and handle counts every 10 minutes, stuck processes, dead links, and the run's end line.

## Linux, first pass (2026-11-24, laptop, release test build 969f071, own empty data root)

The test window ran from ~/src/qanuvi with XDG_DATA_HOME=~/qc-scratch/st1/xdg (an empty data folder, nothing copied
from the installed app), started through a login shell so its PATH matches a desktop start. Scratch projects
~/qc-scratch/st1/proj (git) and ~/qc-scratch/st1/nogit. Paid agent: Claude Code, Sonnet, about $0.15 in all.

Fixed during the pass (969f071, driven again on the rebuilt window unless noted):
- f1: after Ctrl+Shift+F the sidebar stayed on Search; its tabs and Ctrl+Shift+G did nothing until a restart.
- f2: the first click on a change opened Changes with "Choose a file to see its changes" (the work area's copy of
  the Git view cleared the choice when it mounted).
- f3: the Git view was wider than the sidebar: Refresh, Commit and error lines went past its edge.
- f4: a commit with no git name and e-mail showed git's raw text; the list stayed stale after it (files staged,
  shown unstaged), and "Stage all and commit" then did nothing (it stopped on the old error). Now plain words,
  a refresh after any failed action, and the old error cleared first. (Unit-tested; the plain words not driven.)
- f5: closing a terminal tab parked its shell instead of ending it (shells piled up until the window closed).
- f6: "Stop this command, read my message now" left the stopped edit's card with live Approve and Decline.
- f7 (after 969f071, built, not driven): with the terminal maximised, opening a change, a file, the preview or the
  plan showed nothing; the terminal now steps back for them.

Seen, not fixed (for ST2 or later):
- s1: flow 2's steps read well (exact install command shown, sign-in steps plain), but no free model was set up:
  Ollama needs sudo and a 2-9 GB download, OpenRouter the owner's key. Flow 8 waits for one of them. The wizard also
  offers to install Claude Code when it is already installed and connected.
- s2: a file over 10 MB is refused ("not opened in the editor"); VS Code opens it with heavy features off.
- s3: no file watcher: files made outside Qanuvi (terminal, git, other tools) appear in the tree and the Git list
  only after Refresh (the Git badges poll every 15 s).
- s4: maximise the terminal and the focus leaves it (typing goes nowhere until a click).
- s5 (later): programs on a PATH set in ~/.bashrc only (nvm, fnm) are not found by a desktop-started Qanuvi; the
  desktop session reads ~/.profile. The first "Node.js missing" seen here came from starting the window over SSH.
- The system folder picker (GTK) ignores a typed path ending in "/"; picking the folder works. Not Qanuvi's.

Flows 2 and 8 (2026-11-25, same build plus f7 at 113fa70): the owner pasted an OpenRouter key into the wizard; Qanuvi
checked it and picked three free models (nemotron-3-ultra-550b, nemotron-3-nano-omni-30b, openrouter/free) and said
"Done" with what it set. A new session used "Qanuvi agent · Auto": nemotron-3-ultra read src/util.ts and answered
correctly, $0. The hand-over when a model rests could not be caused on purpose (covered by unit tests only). f7
driven: with the terminal maximised, Ctrl+P → util.ts brought the editor back.
- s6: a plain question ("Read ... and tell me ...") was labelled "A change:" when the model was picked; it should
  read as a question.

## Linux, flows 11-20, 27, 30 (2026-11-25, laptop, release test builds of a3c6c54, then b450604 + 600accc)

Same set-up as the first pass (own data root, scratch ~/qc-scratch/st1/proj). Paid agent Claude Code: Sonnet for chat
and the planner, Haiku for the steps, Fable for the check; about $0.90 in all. Step 4a went to the free Qanuvi agent
(team "paid decides, free works").

Fixed during the pass (driven again on the rebuilt window unless noted):
- f8: "Ask the agent to write one" wrote DEVELOPMENT-TASKLIST.md, but the Plan panel kept saying "No task list" until
  Refresh. With no plan found, the panel now looks again whenever a chat of the project stops working. (Built, not
  driven: the scratch project already has its plan.)
- f9: a pause that landed while the agent was ticking its step off said "Paused in step 4a · This run: 0 of 1 done"
  with the step done in the file. The step now counts, and the run pauses after it. Scenario test; not caught live
  again (it needs the pause in the last seconds of a step).
- f10: step model boxes showed raw ids ("claude-haiku-5-5") once a step chat on the free agent was shown: they listed
  the chat's agent's models, not the run's.
- f11: Resume right after a live pause began a new step session (the kept session was only picked up after a restart
  or another plan shown). Driven: step 10 paused mid-step, Resume went on in the same session.
- f12: the tray's "Open Qanuvi" did nothing after the program files were replaced under a running master (rebuild;
  the same happens on a package upgrade or update): Linux names the running program ".../qanuvi-master (deleted)" and
  everything started from that path failed (the app, the Quit question, agent helpers). All callers now use the live
  path. Driven: close the window, tray Open brings it back at the same place.

Seen, not fixed (for ST2 or later):
- s7: the left navigation keeps the last page lit (Memory, Plans) while a chat is shown.
- s8: the check took a TypeScript compiler from another scratch project on the machine
  (~/qc-scratch/y11/.../tsc) and the run's facts now depend on that folder; a project with no checker of its own gets
  a borrowed one without saying it could go away.
- s9: the free agent's step chat shows "Manual" while the run is in Auto (by design the run's rules answer its asks,
  but the chip reads as if the user must approve). The header "Resume" opens the run settings, whose button then says
  "Start". A headless browser a step opened (profile /tmp/qanuvi-browser-...) outlived the run and the master's Quit.
- s10: Quit asks plainly ("stops 1 plan run ... can be resumed afterwards") and the next start shows "Stopped when
  Qanuvi was quit. Resume goes on from here."; Resume finished the step in a new session. A "go on next time" choice
  that resumes by itself is not built. The step chat kept "Waiting for the step's background command" after the quit.
  Claude Code's own background command (sleep) outlived the quit until it ended.
  Built 2026-11-25 (owner: build it): with plan runs going, Quit asks "Keep running · Quit · Go on next time" (the main
  button); the master ends with the note "quit-later" and its next start resumes the runs by itself after 10 s,
  whatever "Start Qanuvi when Windows starts" says, with an Inbox note "Plan run resumed on its own". Driven on the
  laptop: tray Quit during step 12 (sleep 60), Go on next time, start again: step 12 went on in a new session and
  was done; the system yes/no box (no Qanuvi window can open) still means plain Quit.
- s11: status bar: account opens its menu, cost opens Agents, the branch opens Changes, but the sidebar stays on Home,
  so "Choose a file to see its changes" has no list beside it.
- Flow 18 (queue) is R3, never built; owner 2026-11-25: it belongs to the later "Tasks" feature. Owner 2026-11-25: build
  the "go on next time" Quit choice (s10).

## Linux, flows 21-23, 28, 29, 32 (2026-11-25, laptop, release test build of 3e199be, then with ba1 + ba2)

Flows 21 and 32 ran a copy laid out like a user's own install (~/qc-scratch/st21/home/.local/lib/qanuvi, not a
target/ test build), fully apart from the installed Qanuvi on the laptop: its own HOME (data, Documents backups,
autostart, menu entry), its own session bus (an installed-like copy takes the single-instance lock, so on the real bus
it handed over to the installed app and quit), QANUVI_INSTALL_HOME on the test home, and a localhost update channel
signed with a throwaway key (QANUVI_UPDATE_TEST_KEY). Flows 22, 23, 28 and 29 used two target/release test windows on
their own data roots (st1 and st22). Paid agent: one short Sonnet chat on the paired machine, $0.05.

Fixed during the pass:
- ba1: Settings → Backups of This PC (and "Back up now", Restore, Remove) used the master's own default folder
  (~/qanuvi-backups on Linux, Documents/"Qanuvi master backups" on Windows), while the daily backup of the app's data
  goes to the app's folder (Documents/"Qanuvi backups"). The page listed another folder's backups, "Last backup" named
  one that was not in the list, and "Back up now" wrote beside the master backups without the window storage. Since
  the M5a move every installed app had it. The master's backups calls now take the same place as the daily job.
  Rust test added (a master on the app's data shows the app's folder); driven: the page shows the Documents folder and
  its backups, Back up now lands there.
- ba2: a booked restore was named by its folder ("qanuvi-backup-2026-11-24-230637", the UTC date: a day off at night)
  while the list shows local times; it now reads "The backup from 11/25/2026, 5:06:37 AM" (driven, then cancelled).
  The note after a restore ("Restored the backup qanuvi-backup-... kept as ...-before-restore") is written by the
  master, which knows UTC only, and still names the folders (s14).

Seen, not fixed (for ST2 or later):
- s12 (flows 21, 32): Settings → Updates shows the right version before and after; Check now found 0.1.99, Download
  checked the signature, Restart to update put it in (0.1.48 kept as .prev, menu entry, link, icon) and the window came
  back at its place on 0.1.99 with app and master from the new install. Going back has no button: a user unpacks the
  previous tarball (here the published 0.1.47) over it; 0.1.47 opened 0.1.99's data with project, branch and settings
  intact (no schema change since 0.1.26, database version 13). A Linux user going back has no written steps. The
  Updates card says "Channel dev" while a test channel is set. With an empty HOME the Agents card read "Claude Code
  ... not signed in yet" under a green "Connected" badge.
- s13 (flows 22, 23): the second window turned remote access on ("Connected to the relay (through port 443)": a new
  link's first road; the relay names its own port 47443 for the next one, which answers from the laptop) and showed a
  code; the first window paired with it and connected "directly on the home network". The switcher showed it; its
  data folder, project, file (Ctrl+P), terminal (its own folder and environment) and a chat ("pong") worked; This PC
  and back kept the file, chat and terminal. The sidebar Inbox count is all machines' (by design, C2). A headless
  master has no command to turn remote access on or make a pairing code; it is done from a window viewing it.
- s14 (flow 28): Back up now, a project added, Restore at the next start, Restart: the project was gone, the chat
  kept, a "before restore" copy beside the others. All data roots of one kind share one default backups folder: the
  list shows backups of other data folders (other test data here; on a real install, the app and one master are apart
  by design) with a Restore button each, and "the last 7 are kept" counts them all together.
- s15 (flow 24, laptop test copy paired with the installed Home PC 0.1.48, owner moved the Wi-Fi): fixed nb1 (a
  Wi-Fi blip marked the relay's own port blocked for 6 h), nb2 (a direct link waited 45 s after a network change)
  and nb3 (a link that ended by itself waited for the minute's round: away up to a minute after the network came
  back). After a9440ac: Wi-Fi off 36 s and on, direct 5 s after the network; to the hotspot, relay 5 s after; back
  home, direct 1 s after. The road book kept the own port (skip_until 0). Not checked: a drop longer than 40 s.
- s16 (flow 25, laptop test window viewing a server, Wi-Fi off about 60 s): found c1 (the "connection lost,
  reconnecting" cover stayed about 14 s after the link was back: it waited for the window's own next try) and c2 (a
  terminal on the server ended with the connection). Fixed 1bc149c + 06ae772 (owner: c2 in ST): a shell outlives
  its connection for 5 minutes, typing waits, the window takes it over when the machine is back. Driven with a test
  master from 06ae772 on This PC ("This PC test", its own data): link back direct 3 s after the network, cover gone
  within 2 s, the terminal came back with every line printed meanwhile and kept counting. A master before 06ae772
  still ends the shell (the window then says it ended).
- s17 (flow 26, laptop asleep about 3 minutes, window showing "This PC test" with a counting terminal): link back
  direct 2 s after the Wi-Fi rejoined; the terminal came back with the lines printed meanwhile and still counted 2
  minutes later. Found before it (after s16): the server noticed the laptop's old link gone 80 s late, after the
  window had taken the shell over on the new one, and took its output away; fixed fe9b54f (only the connection
  that holds a terminal orphans it). No plan run was going during the sleep (runs not checked here).
- s18 (flow 31, Linux, 2026-11-26): a new user simulated without sudo: an empty HOME, its own session bus, a bare
  system PATH (no Node, Rust or Claude Code), the release tarball unpacked into ~/Qanuvi and started from there.
  First start fine (no errors, knowledge note in the Inbox); with a project "Get AI help for free" is offered; the
  terminal works on the bare PATH. Fixed n31a ecab8ce (driven): "Set up free AI" opened Agents with "Install Claude
  Code" on top and no wizard; it now opens Models and providers with the wizard. Seen: a tarball user has no menu
  entry and a copy outside ~/.local/lib/qanuvi until the first update installs one there (proposal for the owner:
  offer "Install for this user" on first start from elsewhere); backup folders are named in UTC (sorting, by design).
  Not driven: the .deb installer (needs sudo), a real second user account. Owner said yes to the proposal: built
  b08fdcd (s18 Install for this user: asked once in the Inbox; yes copies the unpacked folder in with the update
  helper and starts again from there; unit-tested, the copy run for real on Linux; not driven in a window yet).
  Found while clearing the laptop's full disk (2.1 GB free): bk1, daily backups copied test-builds (4.7 GB each),
  now left out with updates and web caches; tb1, test builds' kept output never went, now removed at start when the
  project is gone or unused 14 days (both b08fdcd).
- s19 (Windows block, This PC, 2026-11-26, owner away): release test build of ecab8ce on its own empty data root
  (APPDATA and LOCALAPPDATA pointed at E:qc-scratchwinst), scratch git project, Claude Code on Sonnet/Haiku,
  $1.37 spent. Pass: 1, 3-7, 9-15, 19, 27 (preview), 28 (Back up now), 29, 30; 17 pause and resume (the step wrote a
  progress note and resume went on at add5; Stop was not caught, the steps finished first). Fixed w1 c4a8a03 (driven):
  Ctrl+C in a terminal stopped nothing (ping -t, a dev server): the master starts in a new process group, which turns
  Ctrl+C off for every shell it starts; now turned on again for its children (the master ignores it itself). The
  earlier test note "SendKeys Ctrl+C does not reach a Windows console shell" was this bug. Seen: the test build's
  desktop notification is titled "Windows PowerShell" when it was started from PowerShell; a held step's question
  quotes a long shell command (heavy for a normal user); the Git list refreshes every 15 s (s3). Not driven on
  Windows: 2 and 8 (no free model set up), 16, 18 (later Tasks), 20, 21-26, 31, 32 (installer, update channel,
  pairing and network flows: need the installed app or the owner).
