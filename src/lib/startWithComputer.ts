/**
 * M5b: the name of This PC's start setting ("Start Qanuvi when Windows starts"), so it is never read as the Qanuvi
 * account, as "...when I sign in" could be. The window runs on This PC, so its own system names it.
 */
export function startSettingLabel(userAgent: string = navigator.userAgent): string {
  return /Windows/i.test(userAgent) ? "Start Qanuvi when Windows starts" : "Start Qanuvi when this computer starts";
}

/** The offer is shown for a run that a shutdown cut off, while the setting is off and not turned away for good. */
export function offerStartWithComputer(stoppedBy: string | undefined, on: boolean | null, off: boolean): boolean {
  return stoppedBy === "shutdown" && on === false && !off;
}
