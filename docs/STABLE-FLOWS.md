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
| 1 | First start, empty data | window opens, setup wizard offered, no error lines | - | - |
| 2 | Setup wizard, free AI | OmniRoute / OpenRouter / Ollama steps say what they do; a free model answers | - | - |
| 3 | Open a project, file tree, editor | open, edit, save, undo; big file opens fast | - | - |
| 4 | Search and go to symbol | Ctrl+P, Ctrl+T, find in files | - | - |
| 5 | Git panel | changes, diff, commit on a scratch repo; no-git folder works | - | - |
| 6 | Terminal | open, type, colours, resize, paste, close; survives switching projects | - | - |
| 7 | Chat with the paid agent | answer streams, tool rows settle, cost shown | - | - |
| 8 | Chat with a free model (Qanuvi agent) | answer, a tool call, a hand-over when a model rests | - | - |
| 9 | Approve and revert an edit | Manual mode asks; Revert puts the file back | - | - |
| 10 | Message while the agent works | taken as a chat, not a restart | - | - |
| 11 | Memory page | add, Try it, suggestion Keep/Dismiss | - | - |
| 12 | Write a plan with the planner | plan file made, steps readable | - | - |
| 13 | Check before the run | card lists needs plainly; answers kept | - | - |
| 14 | Plan run, two steps, real agent | both done, honest end line, report | - | - |
| 15 | Held step and the Inbox | question in plain words; answer from Inbox goes on | - | - |
| 16 | List to look at | half-orange step; typed answer becomes a step; All fine clears it | - | - |
| 17 | Stop, pause, resume a run | each says what happened; nothing lost | - | - |
| 18 | Queue | add, reorder, next task starts after a run ends | - | - |
| 19 | Close to tray and open again | runs go on; window comes back to the same place | - | - |
| 20 | Quit while a run goes on | asks; "go on next time" parks it; next start resumes | - | - |
| 21 | Updates | Settings shows the right line; update from a test channel restarts on the new version | - | - |
| 22 | Pair a machine | code pairing over the relay; machine in the switcher | - | - |
| 23 | Switch machines | the other machine's projects, chat, terminal; back again | - | - |
| 24 | Direct link and fallback | "connected · direct" at home; relay on a hotspot; back by itself | - | - |
| 25 | Network drop and back | reconnecting line, then connected; no stale lines | - | - |
| 26 | Sleep and wake | links, runs and terminals come back | - | - |
| 27 | Preview and live watch | preview address opens; live mirror on request | - | - |
| 28 | Backups and data folder | daily backup made; restore of one table on scratch data | - | - |
| 29 | Settings | every page opens, scrolls, saves; Save always reachable | - | - |
| 30 | Status bar | account, branch, servers, memory sync, agents, cost: each opens its page | - | - |
| 31 | Fresh install as a new user | installer / tarball on a clean user; first start works | - | - |
| 32 | Go back one version | previous build installs over; data intact | - | - |

ST3, the 24 h run, has its own log. It covers memory and handle counts every 10 minutes, stuck processes, dead links, and the run's end line.
