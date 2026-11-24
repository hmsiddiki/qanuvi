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

## Brain and memory (owner, 2026-11-16)

The picture: the brain belongs to the Qanuvi app itself, the same for every user; memory belongs to the user (account
memory: About me, Tools and models; machine memory: This project). The brain reads a user's memory to work for that
user; the memory never becomes part of the brain. Nothing learned, noticed or researched is a rule: it arrives in the
user's memory as a suggestion until the user sets it. The first step (the three memory parts, and the memory disk
sharing account memory only) is batch D8 in the tasklist; the ideas below come after the core.

- A. Learning suggestions from the user's behaviour (memory side): coding style that satisfies them, solutions they
  like, how they act on certain problems, how they debug, what they pick when planning, what they say to plan runners
  mid-run, what they ask about often, what they ask suggestions for. Suggested only after something repeats, with the
  examples it came from; when unclear Qanuvi asks "about you, or about this project?"; each entry shows when it was
  last seen, newer behaviour wins, and a contradiction with an older entry is shown to the user.
- B. Research as a brain ability: the brain researches what the user asks about often and similar things, so later
  answers are more detailed. What it finds for the user goes into the user's memory as suggestions with sources,
  then the normal path (approve, then share or use). A research budget the user sets; an always-on master (for
  example a VPS) fits well; research questions never carry the user's private details to the web.
- C. Voice commands and talking back to the user, as app abilities of the brain.
- D. How the brain gains knowledge: two ways only, a new Qanuvi update (exists: release channels) or the central
  qanuvi.com system (not designed yet: what it sends down, such as decision rules, model notes and research skills,
  and how it is signed). By default nothing from a user's memory goes up; if it ever does, each user chooses it.
- E. NEEDS REVIEW TOGETHER, NOT DECIDED: the brain instructs the model doing a step from the user's memory, shaped for
  that model (short and explicit for a small local model, more for a paid one). Open: what "adapt" means (shaping
  instructions now, adapters later), which brain decides when there are several machines (each core, or the paid
  decider on top), and how to tell it worked (fewer corrections, fewer blocked steps).

## Tasks (owner, 2026-11-25)

A later feature, "Tasks", for recurring background jobs a machine runs on its own. Examples: several plans in a
queue (the old R3 queue, ST1 flow 18, belongs here), a script that runs at a set time every day, making a report,
and other automation. It is not part of the stable-flows work.
