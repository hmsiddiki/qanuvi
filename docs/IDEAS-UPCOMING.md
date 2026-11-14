# Ideas for later (not development yet)

Ideas the owner wants kept for upcoming work. They are not in DEVELOPMENT-TASKLIST.md and are not built until the
owner moves them there; core work comes first.

## Brain training (owner, 2026-11-14)

Qanuvi's brain (its decisions, methods, model choice, checks and reviews; not model weights) becomes trainable and
upgradable over time, instead of copying other projects' work.

- Results record and a versioned brain: change log, roll back, export to another machine.
- Practice tasks from selected open-source repos: past changes that came with tests are given to Qanuvi on scratch
  copies, graded by the repo's own tests and compared with the real fix. Nothing is copied or pushed. Methodology
  repos give ideas that are rewritten as candidate methods and kept only when they measurably help.
- Learning methods from results; a new brain version replaces the current one only when it does better on practice
  tasks it never learned from.
- Runs while the owner is away (task runner), with a budget, free and local models first. What is learned from the
  owner's projects stays local unless shared. Later, brain updates from qanuvi.com with the user's yes.
- Optional, much later: LoRA adapters for small local models (needs a GPU; permissively licensed repos only).
- First practice repos to propose: well tested, clear history, permissive licence, in PHP, TypeScript and Rust.

## Extensions (owner, 2026-11-14)

Qanuvi gets an extension system; the core offers only hook points (before the planner writes a plan, what a step's
agent receives per job type, the step-done gate, the review after a step, run start and end). Text-only extensions
first (instructions, no code run), installed from a git address or a folder, on or off per project, updated with the
user's yes, licence and source shown.

- First extension: Superpowers (github.com/obra/superpowers, MIT, works with many agents, not only Claude), never
  built into the core. Adapted: brainstorming before planning, plan format with a check line per step, check gate,
  optional tests-first, a debugging method for fix steps, the decider's two-stage review, an optional git worktree
  per run. Qanuvi's own ask sorting is kept instead of "never stop"; "delete code written before tests" only as an
  option; its usage-tracking logo is left out; its MIT notice is kept. Short versions for small and free models; not
  sent twice when Claude Code has the Superpowers plugin.
- The brain measures runs with and without an extension and turns what works into its own methods: the extension is
  the benchmark to beat (owner's aim: Qanuvi's brain self-sufficient and better than Superpowers).
