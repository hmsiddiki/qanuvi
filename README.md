# Qanuvi

A light, agent-first project handler and personal assistant. Free and open source.

Qanuvi replaces a heavy editor and browser setup with one small window: projects, plans and task lists, a code editor,
terminals and a live preview, plus agents that work through a task list step by step while you do something else.

Two core goals: a great, light workspace, and spending fewer tokens alongside it.

- **Light:** a native Rust core (Tauri 2) with the system web view; built to sit on a second screen while you game.
- **Agents do the work, the app does the rest:** Qanuvi searches, indexes, runs tests and prepares a small work packet,
  so paid models (Claude, GPT, Gemini) spend tokens only on the hard part. Small local models take the cheap jobs.
- **A model per step:** each step of a plan can run on a different model, chosen by rules and learned results.
- **Personal layer:** memory, rules, style and learned routing that belong to the user, not to a model; stored
  locally or on the user's own storage (Google Drive, FTP, WebDAV, S3, own server) and synced between machines.
- **Master and handlers:** a PC or a server does the work; a phone, laptop or browser signs in and gives commands.
- **Any project:** PHP, Laravel, ASP.NET, Python, Node, Flutter, Android; Qanuvi can set up what a project needs when the
  user allows it.

## Status

Early development. Working today: projects, file tree, code editor with highlighting for 26 languages, go to file,
project search, terminals, site preview, task list view with a model per step, agent sessions with Claude Code
(approval cards, tokens and cost per session), and a diff view. See [DEVELOPMENT-TASKLIST.md](DEVELOPMENT-TASKLIST.md)
for the work map, [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how the parts fit, and
[docs/DESIGN.md](docs/DESIGN.md) for the look.

## Running it

Needs Node.js 20 or later, Rust (stable) and the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your
system. Agent sessions need [Claude Code](https://docs.anthropic.com/en/docs/claude-code) (the `claude` command).

```sh
npm install
npm run app          # development, with live reload
npm run app:build    # release build
npm run verify       # formatting, type check, lint and all tests
```

Open a folder straight away with `qanuvi <folder>` (or `qanuvi .`), like `code .`.

## Shortcuts

| Keys (Cmd on macOS) | Does |
|---|---|
| Ctrl+P | Go to file |
| Ctrl+Shift+F | Search in the project |
| Ctrl+S / Ctrl+F | Save / find and replace in the file |
| Ctrl+W | Close the file |
| Ctrl+B | Sidebar |
| Ctrl+J / Ctrl+Shift+J | Terminal / maximise the terminal |
| Ctrl+Alt+A / Ctrl+Alt+W | Agent pane / work pane |
