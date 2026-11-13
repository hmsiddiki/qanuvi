/**
 * The planner (Milestone 14): a strong model reads the project read-only (plan mode, lean, with the index tools) and
 * answers with task-list steps. Qanuvi checks them and writes them into the task list itself.
 */
import { api, errorText, type AgentEvent, type Project } from "$lib/api";
import { providerById, type AgentProvider } from "$lib/agents/providers";
import { setupFor } from "./brain.svelte";
import { archivePath } from "$lib/plan/housekeeping";
import { appendSection, extractSteps, lastNumber, plannerPrompt, renumber, sectionTitleFrom } from "$lib/plan/planner";
import type { Step } from "$lib/plan/tasklist";
import { agent } from "./agent.svelte";
import { settings } from "./settings.svelte";
import { memoryContext } from "./memory.svelte";

export interface Preview {
  request: string;
  title: string;
  block: string;
  steps: Step[];
  costUsd: number;
}

/** The planner model to suggest: the second strongest when there is one (strong enough to plan, cheaper than the top). */
export function defaultPlannerModel(p: AgentProvider): string {
  return (p.models[1] ?? p.models[0]).id;
}

class PlannerStore {
  busy = $state(false);
  error = $state("");
  preview = $state<Preview | null>(null);

  /**
   * Asks the planner something and returns its final answer. The session is read-only and is closed afterwards; it
   * shows live in the agent pane.
   */
  ask(
    project: Project,
    prompt: string,
    model: string,
    title: string,
    opts: { agentId?: string; lean?: boolean; quiet?: boolean } = {},
  ): Promise<{ text: string; costUsd: number }> {
    const provider = providerById(opts.agentId ?? settings.value.agent);
    const s = settings.agent(provider.id);
    return new Promise((resolve, reject) => {
      let id: string | null = null;
      let text = "";
      let cost = 0;
      let settled = false;
      // Quiet: nothing shown, but the session's status still reaches the list (else it stays "running" after it
      // answered and was stopped, and the status bar keeps counting it as working).
      const show = opts.quiet
        ? (event: AgentEvent) => {
            if (event.kind === "session") agent.track(event.session);
          }
        : agent.handler(() => id);
      const done = (err?: string) => {
        if (settled) return;
        settled = true;
        if (id) api.agent.stop(id).catch(() => {});
        if (err) reject(new Error(err));
        else resolve({ text, costUsd: cost });
      };
      const onEvent = (event: AgentEvent) => {
        show(event);
        if (event.kind === "line") {
          try {
            const msg = JSON.parse(event.line);
            if (msg.type === "result") {
              text = typeof msg.result === "string" ? msg.result : "";
              if (msg.is_error) done(text || "The planner stopped with an error.");
            }
          } catch {
            // not JSON: ignore
          }
        } else if (event.kind === "session") {
          id = event.session.id;
          cost = event.session.cost_usd;
          if (event.session.status === "idle") done();
          else if (["failed", "ended", "stopped"].includes(event.session.status))
            done(text ? undefined : "The planner did not finish.");
        } else if (event.kind === "exit") {
          done(text ? undefined : "The planner did not finish.");
        }
      };
      if (!opts.quiet) agent.watch(null);
      // The planner gets the relevant part of the user's memory too.
      memoryContext(project.id, prompt)
        .then((context) =>
          api.agent.start(
            project.id,
            null,
            prompt,
            model,
            "plan",
            s.baseUrl,
            s.effort,
            onEvent,
            opts.lean ?? true,
            s.indexTools,
            false,
            context,
            provider.id,
            setupFor(provider.id),
          ),
        )
        .then((session) => {
          id = session.id;
          agent.track(session);
          agent.rename(session.id, title);
          if (!opts.quiet) agent.watch(session.id);
        })
        .catch((e) => done(errorText(e)));
    });
  }

  /** Plans a request and keeps the result as a preview for the user to accept. */
  async plan(project: Project, file: string, request: string, model: string): Promise<void> {
    // The paid agent plans when it is active (24e), whichever agent new sessions use.
    const provider = providerById(agent.runProviderId);
    this.busy = true;
    this.error = "";
    this.preview = null;
    try {
      const title = sectionTitleFrom(request);
      const answer = await this.ask(
        project,
        plannerPrompt(request, file, provider.models),
        model,
        `Planner: ${title}`,
        {
          agentId: provider.id,
        },
      );
      const { block, steps } = extractSteps(answer.text);
      if (steps.length === 0) throw new Error("The planner's answer had no steps. Its reply is in the agent pane.");
      this.preview = { request, title, block, steps, costUsd: answer.costUsd };
    } catch (e) {
      this.error = errorText(e);
    } finally {
      this.busy = false;
    }
  }

  /**
   * Writes the previewed steps into the task list as a new section, numbered after the existing steps (and the archived ones). Returns the
   * line of the first new step (to start a run there).
   */
  async accept(project: Project, file: string, sectionTitle: string): Promise<number> {
    const p = this.preview;
    if (!p) throw new Error("Nothing to add.");
    const current = await api.files.read(project.id, file).catch(() => null);
    const text = current?.text ?? "";
    // Numbers that are only in the plan's archive (24j) are taken too: a new step never repeats one.
    const archived = (await api.files.read(project.id, archivePath(file)).catch(() => null))?.text ?? "";
    const numbered = renumber(p.block, Math.max(lastNumber(text), lastNumber(archived)) + 1);
    const next = appendSection(text, sectionTitle || p.title, numbered);
    await api.files.write(project.id, file, next, current?.hash ?? null);
    this.preview = null;
    const added = (await import("$lib/plan/tasklist")).parsePlan(next).steps;
    return added[added.length - p.steps.length]?.line ?? 0;
  }
}

export const planner = new PlannerStore();
