/**
 * The models the user chose for Qanuvi's own agent (Milestone 24), as the agent list shows them. Kept apart from the
 * settings so the agent descriptions can read it without importing the settings (which import them).
 */
import type { AgentModel } from "./providers";

export const own = $state<{ models: AgentModel[] }>({ models: [] });
