# 24y: a big plan runs by itself (proposal, 2026-11-20)

Status: AGREED by the owner (2026-11-20, "ok do whats best"); part 1 done, part 2 done; part 3 agreed 2026-11-23 (Y8, Y7, Y6, Y9); Y8 + Y7 built 2026-11-23. Build order: part 1 = Y1 + Y2, part 2 = Y3 + Y4 + Y5,
part 3 = Y6 to Y9; each part driven live on a scratch project.

## The goal in one line

A plan the size of R1 to R5 (about 25 steps over Rust and TypeScript, tests, test builds, a live check) runs from
start to end without the user, except the final merge and publishing. R1 to R5 itself is done (built by hand); it is
only the yardstick.

## Two kinds of users, one runner

The runner is the same for everyone. What differs is how much it asks.

- **Normal users** (not experts, easily distracted): at most one short card before the run, in plain words, with a
  ready answer on every line. "Do what is best" is a full answer. During the run Qanuvi asks nothing; anything that
  needs them waits in one list in the Inbox, and the run goes on with the other steps.
- **Advanced users**: the same card has "Details", where every rule can be set: what commands may run, which checks a
  step must pass, how test builds are made, git or not. Their settings are kept per project, so the next plan asks
  nothing at all.

The card before the run (normal user):

> **Before this plan runs alone**
> While it works, Qanuvi may: change files in this project, run its tests and builds.
> It never: publishes, deletes outside the project, uses your passwords, or changes your main branch.
> Keep the work on its own git branch? Yes / **No, Qanuvi keeps its own copies** (pre-chosen)
> [Start] [Details]

## What is built (in this order)

**Y1. Run rules (standing answers).** One place per project that says what a run may and may never do. The pre-run
check and every ask read it first: a question the rules already answer is decided by Qanuvi and noted, never sent
to the user. Defaults are the card above. Advanced users edit them under Details.

**Y2. Commands that may run.** A list per project of command patterns the run may use without asking (made from the
project itself: its test and build commands, package.json scripts, cargo test, git on the plan's own branch). The
fixed never-list stays (risky commands such as deleting outside the project, force pushes, publishing). A command
that is neither allowed nor forbidden does not stop the run: that step is set aside with the reason, and the run
goes on.

**Y3. Undo a failed step safely.** Only after the user said yes on the card: a git branch per plan and a commit per
step; main is never touched, merging stays the user's. No git, or the user said no: Qanuvi's own snapshots (already
used by reviews) undo the step instead. A failed step is undone (only its own changes), set aside, and the steps
that do not depend on it go on.

**Y4. Checks per step.** A step's Done line can name its checks ("Check: npm test -- notes"). The runner runs them
after the step, gives the agent a few fix rounds, and undoes and sets the step aside if they still fail.

**Y5. End report.** Done; set aside and why; waiting for you (with the one action each needs). Plain words, in the
Run panel and the Inbox.

**Y6. One review list.** Checks that need eyes (how a page looks, a wording choice) go to one list in the Inbox
instead of holding the run.

**Y7. Health watch.** If the proxy or a model provider is down, the run pauses, says so once, and goes on by itself
when it is back (usage-limit waits already work this way; this adds the proxy and providers).

**Y8. Test builds of the app.** A project can describe how its app is test-built: built into its own folder, started
with its own data on a scratch copy, checked by a command, closed after; never over the installed app. A memory
check comes first (builds wait when free memory is low). Qanuvi's own repo gets this recipe first.

**Y9. The benchmark.** A plan of R1 to R5's size runs on a scratch copy. Pass: it ends alone, and the report is
right.

## Owner decisions (2026-11-20)

1. **Where the run rules live:** in Qanuvi, per project (nothing extra in the user's files). A plan file may add a
   "Run rules" section that wins for that plan only.
2. **Where the card shows:** at the top of the Run panel's existing "Before the run" box (no new screen). In full
   only on the first run in a project; after that one line, "Run rules: as before · Change". Start run and Accept all
   recommended fill it in too.
3. **The branch question:** pre-chosen answer is "No, Qanuvi keeps its own copies" (git is the user's; undo still
   works through snapshots). Yes is kept per project once chosen. A project without git never sees the line.
4. The other defaults on the card stand as written above.
5. **The benchmark (Y9) runs on one machine, the Linux laptop** (owner, 2026-11-23): the runner is the same core
   on both systems, and Windows is used and driven live every day here. It runs on a scratch copy on the laptop,
   started and watched from the window on This PC (the laptop as the shown machine), while work goes on here;
   the results are checked after. The owner's Max plan covers it; no need to hold work here back.
6. **Part 3 order and benchmark** (owner, 2026-11-23): Y8 -> Y7 -> Y6 -> Y9. The benchmark re-runs the R1 to R5
   plan on a scratch copy of Qanuvi taken from the commit before R1, so the right result is known. Paid runs are
   fine (Max plan): run them without asking.

## Y8 in detail (owner "ok do", 2026-11-23; built)

1. **The recipe** is in the project's settings (Project settings, "Test build"): Build, Start (optional), Check,
   and the free memory it waits for (GB, default 4). A plan file's own "## Test build" section (lines "Build:",
   "Start:", "Check:", "Memory: 6 GB") wins for that plan, so a scratch copy needs nothing set up. Qanuvi fills in
   `{out}` (the project's own build folder in Qanuvi's data, kept so later builds are quick) and `{data}` (fresh
   each time, removed after).
2. **When:** a step with the line "Check: test build". It runs after the step's other checks pass, with the same
   fix rounds as Y4; still failing, the step is undone and set aside.
3. **Memory first:** while free memory is under the recipe's GB the build waits (looked at every 30 s; the Run
   panel says "test build waits for N GB of free memory"); after 20 minutes the step is set aside with the numbers.
4. **Never over the installed app:** a recipe that names Qanuvi's data folders or the running Qanuvi's folder is
   refused before any work, as is one with a command on the run rules' never-list. A recipe that starts the app
   but has no Check, or no recipe at all, is refused the same way.
5. **Closed after:** whatever Start runs is stopped (its whole process tree), pass or fail.
6. **Qanuvi's own repository** gets its recipe when added or on "Look again": `cargo build --release --bin
   qanuvi-master --target-dir {out}`, start `{out}/release/qanuvi-master core --data {data}`, check
   `{out}/release/qanuvi-master status --data {data}` (status ends with exit code 1 when the master is not running).
   Unattended runs build the headless master only; the window build stays with live drives.

## Y7 in detail (owner "ok do Y7", 2026-11-23; built)

1. **What counts as down:** only the agent's own error end (as for usage limits): a server error (500, 502, 503,
   504, 529), "overloaded", or the connection failing (refused, reset, timed out, fetch failed). A request the
   provider refused (400, 401) is not an outage, and nothing the step's work says or reads counts.
2. **The proxy** the paid agent goes through (the run's own address, else the one Claude Code's settings name, as
   the headroom wrap sets) is looked at before every paid try: its port must answer. While it does not, the try
   waits (looked at every 15 s).
3. **After an outage** the try is not counted (the step keeps its tries and models). The run waits 1, 2, 5, then 10
   minutes (the proxy, if any, must answer too) and tries the step again.
4. **Said once:** one notice per outage ("The model provider is not answering: the run waits and goes on by itself
   when it is back."); the Run panel's status line says what is down while it waits; each wait goes into the
   run's decisions with how long it took. Pause and Stop work during the wait.
5. **Never stuck:** a step that met outages for 3 hours is set aside ("the model provider kept failing for 3
   hours"), and the run goes on with the other steps.

Already decided and built (24u part 1, setaside.rs), not a question: a step that needs the user mid-run is set aside
with its question in the Inbox, the steps that depend on it are set aside too, and the run goes on with the rest.
24y keeps this as it is.
