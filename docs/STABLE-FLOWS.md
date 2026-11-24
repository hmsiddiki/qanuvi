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
| 1 | First start, empty data | window opens, setup wizard offered, no error lines | - | pass |
| 2 | Setup wizard, free AI | OmniRoute / OpenRouter / Ollama steps say what they do; a free model answers | - | pass (OpenRouter) |
| 3 | Open a project, file tree, editor | open, edit, save, undo; big file opens fast | - | pass (s2, s3) |
| 4 | Search and go to symbol | Ctrl+P, Ctrl+T, find in files | - | pass after f1 |
| 5 | Git panel | changes, diff, commit on a scratch repo; no-git folder works | - | pass after f2-f4 |
| 6 | Terminal | open, type, colours, resize, paste, close; survives switching projects | - | pass after f5 (s4) |
| 7 | Chat with the paid agent | answer streams, tool rows settle, cost shown | - | pass |
| 8 | Chat with a free model (Qanuvi agent) | answer, a tool call, a hand-over when a model rests | - | pass (hand-over not driven) |
| 9 | Approve and revert an edit | Manual mode asks; Revert puts the file back | - | pass |
| 10 | Message while the agent works | taken as a chat, not a restart | - | pass after f6 |
| 11 | Memory page | add, Try it, suggestion Keep/Dismiss | - | pass |
| 12 | Write a plan with the planner | plan file made, steps readable | - | pass after f8 |
| 13 | Check before the run | card lists needs plainly; answers kept | - | pass (s8) |
| 14 | Plan run, two steps, real agent | both done, honest end line, report | - | pass |
| 15 | Held step and the Inbox | question in plain words; answer from Inbox goes on | - | pass |
| 16 | List to look at | half-orange step; typed answer becomes a step; All fine clears it | - | pass |
| 17 | Stop, pause, resume a run | each says what happened; nothing lost | - | pass after f9-f11 (s9) |
| 18 | Queue | add, reorder, next task starts after a run ends | - | not built (R3 open) |
| 19 | Close to tray and open again | runs go on; window comes back to the same place | - | pass after f12 |
| 20 | Quit while a run goes on | asks; "go on next time" parks it; next start resumes | - | pass; "go on next time" not built (s10) |
| 21 | Updates | Settings shows the right line; update from a test channel restarts on the new version | - | - |
| 22 | Pair a machine | code pairing over the relay; machine in the switcher | - | - |
| 23 | Switch machines | the other machine's projects, chat, terminal; back again | - | - |
| 24 | Direct link and fallback | "connected · direct" at home; relay on a hotspot; back by itself | - | - |
| 25 | Network drop and back | reconnecting line, then connected; no stale lines | - | - |
| 26 | Sleep and wake | links, runs and terminals come back | - | - |
| 27 | Preview and live watch | preview address opens; live mirror on request | - | preview pass; live mirror not driven |
| 28 | Backups and data folder | daily backup made; restore of one table on scratch data | - | - |
| 29 | Settings | every page opens, scrolls, saves; Save always reachable | - | - |
| 30 | Status bar | account, branch, servers, memory sync, agents, cost: each opens its page | - | pass (s11) |
| 31 | Fresh install as a new user | installer / tarball on a clean user; first start works | - | - |
| 32 | Go back one version | previous build installs over; data intact | - | - |

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
- s11: status bar: account opens its menu, cost opens Agents, the branch opens Changes, but the sidebar stays on Home,
  so "Choose a file to see its changes" has no list beside it.
- Flow 18 (queue) is R3, never built. Flows 21-26, 28, 29, 31, 32 are next; 24-26 and 31 need the owner (hotspot,
  network off, sleep, a clean user account).
