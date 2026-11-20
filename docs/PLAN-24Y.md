# 24y: a big plan runs by itself (proposal, 2026-11-20)

Status: PROPOSAL, waiting for the owner's yes. Nothing here is built yet.

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
> Keep the work on its own git branch? **Yes (recommended)** / No, Qanuvi keeps its own copies
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

## Three questions for the owner (with my recommendation)

1. **Where the run rules live.** In Qanuvi, per project (recommended: nothing extra in the user's files; the plan
   file can still add a "Run rules" section that wins for that plan). Or only in the plan file.
2. **The default card.** Are the defaults above right for normal users (may change files and run tests and builds;
   never publish, delete outside, use passwords, touch main; branch recommended)?
3. **Something needs the user in the middle of a run.** Set that step aside and go on (recommended), or hold the
   whole run until they answer.
