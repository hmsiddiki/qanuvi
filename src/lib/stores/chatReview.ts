/** Paid agent on top (24e): it reviews what a free model changed in a chat turn. */
import { errorText, type Project } from "$lib/api";
import { paidAgent } from "$lib/agents/providers";
import { parseReview, reviewPrompt } from "$lib/core/decider";
import { agent } from "./agent.svelte";
import { changesSince } from "./changes";
import { defaultPlannerModel, planner } from "./planner.svelte";
import { settings } from "./settings.svelte";
import { freeDecide } from "./brain.svelte";

/**
 * Reviews the free turn that just ended. Nothing changed (a question): no review. Accepted: a line says so. Not
 * accepted: a line says why and the paid agent takes the request over.
 */
export async function reviewFreeTurn(project: Project): Promise<void> {
  const turn = agent.freeTurn;
  agent.freeTurn = null;
  const paid = paidAgent();
  if (!turn || !paid || turn.projectId !== project.id) return;
  const parts = await changesSince(project.id, turn.before);
  if (!parts.length) return;
  const model = settings.agent(paid.id).model || defaultPlannerModel(paid);
  try {
    const { text, costUsd } = await planner.ask(
      project,
      reviewPrompt({ title: turn.request.slice(0, 2000), details: [] }, parts.join("\n"), ""),
      model,
      "Decider: review a free change",
      { agentId: paid.id, lean: true, quiet: true },
    );
    const review = parseReview(text);
    const cost = costUsd ? `, $${costUsd.toFixed(4)}` : "";
    if (review.ok) {
      agent.notice(`${paid.name} reviewed the change: accepted (${review.why}${cost}).`);
      return;
    }
    agent.notice(`${paid.name} reviewed the change: not accepted (${review.why}${cost}). ${paid.name} takes it over.`);
    await agent.handToPaid(project.id, paid.id);
  } catch (e) {
    // The paid agent cannot answer now: the free models review instead (24e).
    try {
      const f = await freeDecide(
        "You review code changes. Answer with JSON only.",
        reviewPrompt({ title: turn.request.slice(0, 2000), details: [] }, parts.join("\n"), ""),
      );
      const r = parseReview(f.text);
      agent.notice(
        `${paid.name} could not review the change (${errorText(e)}); ${f.by} reviewed it: ${r.ok ? "accepted" : "not accepted"} (${r.why}).${r.ok ? "" : " Check it yourself."}`,
      );
    } catch {
      agent.notice(`${paid.name} could not review the change (${errorText(e)}); check it yourself.`);
    }
  }
}
