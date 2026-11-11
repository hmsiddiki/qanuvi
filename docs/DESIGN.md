# Qanuvi design: look and feel

Reference: [design/reference-layout.png](design/reference-layout.png), a screenshot the owner picked for the feel
(dark, calm, rounded, three panes, agent in the middle, live result on the right). It is a visual reference only:
Qanuvi uses its own name, icons and branding, and copies no artwork or text from it.

## Feel

- Calm and dark by default (a light theme comes later). Low contrast chrome, high contrast content.
- Panels are rounded cards floating on a slightly darker window background, with thin, quiet borders.
- Small, tidy type; grey for secondary text, near-white for primary text, colour used sparingly (status dots, project
  icons, one accent for primary actions).
- Nothing flashes or moves without a reason; progress is shown as short quiet lines, not spinners everywhere.
- Light on memory: hidden panes and tabs are unloaded, not just hidden.

## Window layout

```
┌───────────────┬──────────────────────────────┬───────────────────────────────────────────┐
│  SIDEBAR      │  AGENT PANE                  │  WORK PANE (tabs)                          │
│               │  title of the current session│  [ editor tabs ... ] [ Preview ] [ Diff ]  │
│  workspace ▾  │                              │ ┌───────────────────────────────────────┐ │
│  Home         │  step lines (quiet, grey):   │ │                                       │ │
│  Inbox      • │   Checking layout            │ │   code editor  /  preview  /  diff    │ │
│  Projects     │   no overflow                │ │                                       │ │
│  Plans        │                              │ │                                       │ │
│  Agents       │  ┌ result card ───────── > ┐ │ ├───────────────────────────────────────┤ │
│               │  └─────────────────────────┘ │ │ TERMINAL  (split under the editor,    │ │
│  PROJECTS     │                              │ │ like VS Code; tabs, resize, hide)     │ │
│  ● qcommerce  │  agent messages              │ └───────────────────────────────────────┘ │
│  ● qanuvi      │                              │                                           │
│  SESSIONS     │ ┌──────────────────────────┐ │                                           │
│  Step 82   •  │ │ Ask Qanuvi...      + ▤ ↗  │ │                                           │
│  Step 83      │ └──────────────────────────┘ │                                           │
└───────────────┴──────────────────────────────┴───────────────────────────────────────────┘
```

### Sidebar (left)
- Top: window controls, a search button, back/forward.
- Workspace switcher (account and workspace name with a dropdown).
- Main items with icons: Home, Inbox (approvals and messages, with a dot when something waits), Projects, Plans,
  Agents, Reminders.
- Section headers in small capitals: PROJECTS (each with a coloured icon and its type), SESSIONS (agent sessions with a
  status dot: running, waiting for you, done, failed).
- Collapsible to icons only.

### Agent pane (middle)
- The session title across the top of the pane.
- Agent output as plain sentences, with step progress as short grey two-line items (what it is doing, the result).
- Result cards: an icon, a title, a one-line subtitle and a chevron; clicking opens the file, page, diff or step in the
  work pane.
- Approval cards with clear buttons (Approve, Refuse, Show me).
- Input box at the bottom: "Ask Qanuvi...", with buttons to attach a file, point at something in the preview, choose the
  model, and send. Voice input comes later.

### Work pane (right, tabbed)
- **Editor is the main tab.** Editor tabs per open file; a setting decides whether tabs follow the project (switching
  project swaps the tab set) or stay open across projects.
- **Preview tab:** a small browser bar (address, reload, device sizes desktop / tablet / phone, pointer to send an
  element to the agent). The preview is unloaded when its tab is hidden.
- **Diff tab:** the agent's changes, file by file, with accept and revert.
- **Terminal panel split under the editor**, like VS Code: several terminals in tabs, drag to resize, hide and show with
  a shortcut, can be maximised.

### Layouts
- Every pane can be resized, hidden or moved; the user saves named layouts (for example "Coding", "Side screen while
  gaming" with only the agent pane and the task list, "Review" with agent and diff).

## Tokens (first draft, tuned when the shell is built)
- Window background `#141416`, panel `#1c1c1f`, raised `#232327`, border `#2c2c31`.
- Text primary `#ececef`, secondary `#9b9ba3`, muted `#6b6b73`.
- Accent `#4f8cff`; status: running `#4f8cff`, waiting `#e5b84b`, done `#4cc38a`, failed `#e5604b`.
- Radius: panels 12 px, cards 10 px, inputs 10 px, buttons 8 px.
- Type: system UI font, 13 px base, 12 px secondary, 11 px section headers in capitals with letter spacing; monospace
  for code and terminal.
- Spacing on a 4 px grid.
