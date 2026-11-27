# The runner baseline

The standard every runner fix and every update is compared with. If a change makes any line below fail, the change is
wrong: it is fixed or dropped, never published. Owner, 2026-11-28.

## The flow (owner's words, in order)

1. **Before the run, the check asks what is needed, cleanly.** The owner answers, or lets Qanuvi decide where Qanuvi
   can.
2. **With "Stop for me: Never" and everything agreed, the run starts.**
3. **It keeps going to the end.** Only something truly important (a secret only the owner has, or a real yes on big
   or risky work) waits for the owner. That question may show at once, but it never stops the other steps; the run
   waits for it only when nothing else is left.
4. **Nothing the agent can judge reaches the owner:** a program, an address, a message typed in a step's chat, a
   command the rules refuse (that step is set aside, the rest goes on), a provider outage or a usage limit (the run
   waits and goes on by itself).
5. **"After every step"** pauses after a whole step, and the next run goes on from the next one.

## What guards each point

All are runner scenarios in `src-tauri/src/plan/runner/scenarios.rs` (`cargo test --lib plan::`, about 2 seconds).

| Point | Scenario |
|---|---|
| 1-3 whole stream | `baseline_the_whole_stream_check_then_a_quiet_run_to_the_end_only_the_important_thing_waits` |
| 1 check settles what it can | `y9_1_a_run_started_without_a_window_gets_the_check_first_and_keeps_what_it_settled` |
| 1 unanswered question | `y10_a_question_nobody_answers_is_decided_after_the_wait_but_a_risky_step_still_waits` |
| 3-4 no guessed holds | `baseline_a_plan_without_secrets_runs_start_to_end_without_asking_the_owner` |
| 3-4 old guessed holds dropped | `a_kept_missing_program_hold_is_dropped_when_the_run_starts_again` |
| 3 blocked step | `a_blocked_step_goes_to_the_inbox_the_run_waits_and_an_answer_lets_it_go_on` |
| 3 a held step asks at once | `baseline_a_held_step_that_needs_a_later_step_still_asks_its_own_question` |
| 1 end check kept whole | `a_long_end_check_command_is_kept_whole` (preflight.rs) |
| 4 a step stops its own server | `a_step_may_stop_a_process_it_started_in_the_project_and_no_other` (run_rules.rs) |
| 4 chat note | `a_note_typed_in_the_steps_chat_is_sorted_first_then_passed_on_and_the_step_goes_on` |
| 4 chat "change" | `a_message_the_free_model_calls_a_change_goes_to_the_working_agent_and_nothing_waits_for_the_owner` |
| 4 refused command | `y24_a_command_the_run_rules_refuse_sets_its_step_aside_with_the_command_named_and_the_run_goes_on` |
| 4 provider outage | `y7_a_provider_outage_is_no_failure_the_run_waits_says_so_once_and_goes_on` |
| 4 usage limit | `a_usage_limit_waits_for_the_reset_and_the_step_is_tried_again` |
| 4 checks that need eyes | `y6_checks_that_need_eyes_go_on_one_review_list_and_the_run_never_waits_for_them` |
| 5 pause after each step | `pause_after_each_step_stops_after_a_whole_step_and_the_next_run_goes_on_from_the_next_one` |

## Rules

- **A bug found live becomes a scenario first.** It must fail on the bug, then the fix makes it pass, and it is added
  to the table above.
- **A scenario is never loosened to let a fix pass,** unless the owner changed that behaviour.
- **`scripts/publish-update.mjs` runs every test before publishing** and refuses when one fails, even with
  `QANUVI_RELEASE_ANYWAY`.
- **Each release is also driven for real on the laptop** with the same small plan. The numbers below must not get
  worse than the last good row.

## Real-run record (laptop, same plan each release)

The plan: `~/qc-scratch/st53/proj/PLAN.md` on the laptop (5 steps: two helpers with tests, a demo page with a server
and a browser check, a release note that needs a secret the project does not have, a README). Run from the master's
terminal with Claude Code in Auto mode, "Stop for me: Never". The right result is 4 steps done and only step 4 asking
(for its secret), with no server left running.

| Build | Steps done | Questions | Stops before the end | Notes |
|---|---|---|---|---|
| 65411be | 3 of 5 | 1 (wrong) | step 5 blocked | The end check written into step 5 was cut at 300 characters with "…", so it could never run. The `kill` for the step's own demo server was refused, the server kept the port. Step 4's own question (the secret) was never asked. |
| 055483b | 2 of 5 | 2 | step 3 blocked | The secret question for step 4 was asked at once (fixed). Step 3's `kill "$pid"` was still refused: the number sat in a variable. |
| 0.1.53 (b8128fa) | 3 of 5 | 2 | none | Step 4 asked at once (right). Step 5 also asked: the end check `... & pid=$!; ...; kill $pid` was refused, so it waited for the owner. No server left. |
| 6403d56 | 4 of 5 | 1 (right) | none | **Target met.** Only step 4 asks: a real post to an outside chat that needs a secret. The end check ran whole and passed; the agent stopped its server by number after the hint; no server left running. Publish as 0.1.54 waits for the owner's yes. |

Scenarios added from these runs: `baseline_a_held_step_that_needs_a_later_step_still_asks_its_own_question`,
`a_long_end_check_command_is_kept_whole`, `a_step_may_stop_a_process_it_started_in_the_project_and_no_other`.

Still open (no stop, only slower): every step first fails twice on the free model ("qanuvi:auto: not finished",
error after 5-20 s, no message kept) before a paid model does it. Each step loses about 30 seconds.

## Where it slipped before

- **Y10 (0.1.45, 2026-11-23):** the check began holding steps for programs and addresses it guessed. The owner's
  sign-in plan stopped on "install qanuvi-browser", which is Qanuvi's own browser tool, not a program. Fixed in
  91d55e3: only missing secrets hold.
- **Before 0.1.53:** a step could not stop a server it had started (the never-list refused every `kill`); the end
  check was cut at 300 characters; a held step that also needed a later step never asked its own question. Fixed in
  055483b and f645809.
- **R2c (2026-11-15):** a chat message the free sorter called a "change" asked the owner "when?" and held the step.
  Fixed in 1c27d83 and b60aec7: the working agent decides.
