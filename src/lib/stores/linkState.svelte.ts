/**
 * ce1: whether the last call found its link gone. An error line that only says the link was gone ("The connection
 * to the master has ended.") is about that moment, not about the work: it shows while the link is still gone and no
 * reconnect notice says so already, and goes by itself once a call gets through again.
 */
import { isLinkError, onCallOk, onLinkGone } from "$lib/remote";
import { switcher } from "./switcher.svelte";

class LinkState {
  /** A call found its link gone, and none got through since. */
  failing = $state(false);
}

export const linkState = new LinkState();

onLinkGone(() => (linkState.failing = true));
onCallOk(() => {
  if (linkState.failing) linkState.failing = false;
});

/** The error text to show: "" for a link error that is over or already said by the reconnect notice. */
export function shownError(text: string | null | undefined): string {
  if (!text) return "";
  if (!isLinkError(text)) return text;
  return linkState.failing && !switcher.held ? text : "";
}
