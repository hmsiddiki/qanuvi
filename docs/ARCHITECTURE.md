# Qanuvi architecture

How the parts fit, as agreed with the owner on 2026-11-10. The build order is in
[DEVELOPMENT-TASKLIST.md](../DEVELOPMENT-TASKLIST.md).

## The parts

| Part | Runs on | Does |
|---|---|---|
| **Qanuvi app, master mode** | A PC (Windows, Mac, Linux) or a Linux server/VPS | All the work: agents, terminals, files, git, tests, the test browser, the code index, the personal layer, local models |
| **Qanuvi app, handler mode** | Phone, another laptop, a browser | Only gives commands and shows results: pick a master and project, run or stop plans, approve, ask, view progress, diffs and screenshots, notifications. Never runs work, keeps no keys or project files |
| **Accounts** | Any Qlopy site | Sign-in with Qlopy's OAuth 2.0 + PKCE for public clients (`includes/rest-oauth.php` in Qlopy core), devices, settings |
| **Relay** | A small Rust service on the owner's hosting or a VPS | Live connection between handlers and masters (masters connect out, so no port forwarding), reminders, push notifications, sync |

One account can have several masters (home PC, office laptop, VPS); each project runs on the master that suits it.
Working offline is not a goal: agents need the internet.

## The brain is the app

```
┌──────────────────────── THE BRAIN (Qanuvi) ───────────────────────────┐
│  Controller (code): task lists, steps, states, retries, approvals     │
│  Personal layer: memory, rules, style, learned routing                │
│  Escalation rules: decided by clear signals, not a model's own guess  │
└──────────┬───────────────────────┬───────────────────────┬───────────┘
           │                       │                       │
   Local models + adapters   Paid model (planner)    Paid model (worker)
   cheap jobs                plans, judges           hard coding
```

- The **controller** is ordinary code: it runs a task list step by step, tracks state, retries, asks for approval and
  never forgets.
- **Escalation** uses clear signals: the kind and size of the job, failed tests, repeated retries, learned success
  rates per model and job type, and the user's rules (for example "payment code always goes to the strongest model").
- **Models are replaceable workers.** For hard work the paid model does the job itself (writes the code, judges the
  result), not just answers a question for a small model.

## Model choice: the right model for each step, to spend fewer tokens

Qanuvi has two core goals: a great, light workspace, and spending fewer tokens alongside it. Choosing the model per
step is how the second goal is met (it started from not being able to change the model in the middle of a long plan),
so it is built early, in Stage 1, next to the workspace itself.

- Every step, and every job inside a step, can run on a different model: the strongest model for money, security and
  hard design; a cheaper model for routine code, docs and tests; a local model for summaries, sorting and small drafts.
- How the model is chosen, in this order: the user's choice on the step (for example `model: fable` in the task
  list); the user's rules ("payment code uses the strongest model", "docs use the cheapest"); learned results (which
  model succeeded on this kind of job, at what cost); a default per project.
- Switching happens between steps without the user being there, and also inside a step: if a cheap model fails twice,
  the step moves up to a stronger model; if a strong model is doing routine work, later similar jobs move down.
- Every run records tokens, cost, time and outcome per model and job type; the app shows them per step, per plan and per
  month, and a monthly budget can make it prefer cheaper models or stop and ask.
- Tokens are also saved by what is sent: work packets (below), only failing test output, short summaries of earlier
  steps instead of whole histories, and a fresh short context for each step instead of one ever-growing conversation.

## Work packets: the app does the cheap work

1. The app collects what the job needs: the step text, the relevant files and lines from its code index, the user's
   rules from the personal layer, the test command.
2. The paid model gets only that packet and makes the change. It can ask the app for more ("give me this file",
   "search for X"); the app answers from its index.
3. The app runs the tests itself and sends back only the failures, not whole logs.
4. Repeat until green; the app marks the step done and stores the result in the personal layer.

Searching, indexing, running tests and trimming logs are done by tools (a ripgrep-style search, a code index, the test
runner) at no token cost.

## Qanuvi's tools for agents (MCP)

Each agent session can get two small MCP servers, both the Qanuvi program itself started in a tool mode:

- `qanuvi --mcp <project>`: the code index (project map, find a definition, outline a file, read some lines, search).
  Read-only.
- `qanuvi --mcp-browser <project> <state file>`: the test browser. A headless Chrome, Edge, Chromium or Brave (whichever
  is installed; `QANUVI_BROWSER` picks one) with its own empty profile, driven over the Chrome DevTools Protocol
  (`browser.rs`, page helper `page.js`). It starts on the agent's first browser call and ends with the session; on
  Windows a job object ends it even when Qanuvi is killed. Answers are short text: what changed after a click, new
  errors, fields and links by number; addresses of project files are shown relative to the project. A screenshot is sent
  only when the agent asks for one. Its last page is written to the state file, which the preview tab can follow.

The preview tab has its own inspector browser (`preview.rs`, the same core) to hand the agent a page as text or an
element the user points at; it stops after 3 idle minutes and when the app closes.

## A master without a window (servers)

The Rust core builds without Tauri (`cargo build --no-default-features`): everything except `desktop.rs` (the
window's commands) and `remote.rs` (the window's connections). Events of agent sessions and terminals go to a `Sink`
(`sink.rs`), which is the window's channel on the desktop and a connection on a server; the data folder comes from
`paths.rs`.

`qanuvi-master` (4 MB) is that core alone, for a Linux server or VPS:

- `qanuvi-master serve` is a background service (127.0.0.1 only; a token in `master.json`, readable by its user
  only). It keeps agent sessions running when no desktop is connected; their output is stored as on the desktop.
- `qanuvi-master --stdio` is what the desktop starts over SSH (`ssh -T -o BatchMode=yes <address> qanuvi-master
  --stdio`). It starts the service when needed and passes messages through. Sign-in is SSH's own (keys); no port is
  opened.
- Messages are JSON lines with the same command names and arguments as the window's own calls (`git_status` with
  `projectId`, …), answered by `master.rs`. In the window, `remote.ts` sends a call to the master that owns its
  project, agent session or terminal, so the editor, git, search, terminals and agents work the same on a server.
- Terminals belong to a connection and end with it; agent sessions belong to the service.

## Personal layer and memory disk

- Model-independent and permanent: memory (facts, decisions, past solutions), approved examples, rules, style, learned
  routing statistics.
- Stored as an encrypted, versioned log of separate entries (each with an id and time), so copies made on two machines
  merge instead of overwriting each other.
- Where it lives, chosen by the user: local only; local plus a remote copy (Google Drive, Dropbox, OneDrive, FTP/SFTP,
  WebDAV, S3-compatible, own server); or remote only. The local copy is the working copy and updates the remote one.
  Encrypted before upload with the user's key.
- Needs no GPU: search over the layer runs on any CPU.

## Memory in practice (Milestone 17)

Entries live in the app's database (`memory.rs`). Each request gets a slice: the rules and style that apply, then the
best-matching entries within a size limit, sent before the user's message but not stored as part of it. Agents can
search for more (`memory_search`) and suggest entries (`memory_suggest`); suggestions wait for the user's approval on the
Memory page, which also shows exactly what a request would get.

## Memory disk (Milestone 18)

Each machine writes only its own encrypted file to the memory disk (a folder, or a folder on a server over SSH); a sync
reads all of them and keeps the newest version of each entry, deletions included, then writes its own file and keeps the
previous one as a version. Key: Argon2id from the passphrase (kept in the system credential store); cipher:
XChaCha20-Poly1305.

## Account (Milestone 19)

One central, optional Qanuvi account at qanuvi.com (a Qlopy site; Google, Microsoft or GitHub sign-in there): OAuth 2.0
authorization code with PKCE in the user's browser, the answer to a one-time listener on
`http://127.0.0.1:47213/callback`, the refresh token in the system credential store, the access token in memory
(`account.rs`).

## Relay (Milestone 20)

A small relay (`qanuvi-relay`) lets devices reach a master from anywhere without opening a port: the master connects
out to it, a device pairs once with a short code (SPAKE2), and after that every connection has its own keys and sealed,
counted frames (XChaCha20-Poly1305), so the relay only passes on bytes it cannot read. Inside, the same master protocol
as over SSH. The window's own computer offers remote access through its master service (`relay_client.rs`).

## Qanuvi's own agent and brain (Milestone 24)

`qanuvi --agent` is an agent loop inside Qanuvi's own program: it speaks the same JSON lines as the command-line
agent, so the session code does not care which one runs. Models are reached through `llm.rs` (Ollama, or any
OpenAI-compatible API such as OpenRouter or OmniRoute); the tools in `agent_tools.rs` stay inside the project folder;
`brain.rs` picks the model for each turn from the user's list (local first for questions, fallbacks on rate limits
and outages, distrust of models that break tool calls) and remembers what it learned in `brain.json`. Keys live in
the system credential store and reach the agent only through its environment.

## Local models and adapters

- Two replaceable local models: a coder (for example a Qwen2.5-Coder size) and a general chat model, through Ollama or
  llama.cpp. One general model is offered as a lighter option.
- Local models never change by themselves. Each gets its own adapter (a small add-on file) built from the personal
  layer's examples; an adapter belongs to one base model only.
- Replacing a model: download the new one, rebuild its adapter from the personal layer, test it, switch. Until then the
  personal layer still gives the new model its context.
- A new adapter version replaces the old one only if it scores better on a fixed set of the user's own tasks.
- Building an adapter needs a GPU somewhere (the user's PC, a rented GPU or a fine-tuning service, with the user's
  permission); using it works on any CPU. A GPU is never required for Qanuvi itself.
- Train on the user's own data and approved results; check each provider's terms before using paid models' raw
  answers as training data, and each model's licence before downloading or recommending it.

## Resources

- No CPU cap: work runs at below-normal priority, so a game or other foreground work gets the CPU first and long tasks
  simply slow down.
- An optional memory limit (Windows Job Objects, Linux cgroups) for users who want one.
- Hidden panes, the preview and the test browser are unloaded when not in use.

## Updates

- Channels `dev`, `beta`, `stable`, each an address with a small manifest (`latest.json`). During development the `dev`
  channel is a local folder on the owner's web server (`www/qanuvi-updates/dev/`); public channels use `https`.
- Every update is signed; the app carries the public key and refuses anything else. The private key never enters the
  repository.
- The user is asked (Update now, Later, Skip this version); updates install between agent steps, never in the middle
  of one, and the app restores its layout, projects, tabs and sessions after the restart.

## Security

- New devices are approved from a trusted device; two-factor sign-in.
- Permissions per device (for example a phone may run plans and approve, but needs an extra confirmation for the raw
  terminal or a git push).
- A master shares only the project folders the user chose.
- Handler-to-master traffic is encrypted end to end, so the relay passes messages without reading them; provider keys
  stay on the master.
- On the master: a visible "being controlled" indicator, an activity log of every remote command, and one-click stop
  and disconnect.
- Anything sent in the user's name (email, SMS, messages) is shown as a draft and confirmed, unless the user set a
  standing rule for that kind of message. System changes (installing software, LEMP setups) are shown first and
  confirmed.
