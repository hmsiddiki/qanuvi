/**
 * Store tests written against This PC's commands (migration M5a). The window now sends them to This PC's master as
 * `remote_call` with master "this-computer"; the master answers the same commands with the same arguments, so a
 * test's `invoke` mock can keep answering the plain command. This unwraps those calls before the mock's own
 * implementation sees them (calls to other masters are left as they are). How the call travels is remote.test.ts's.
 */
import { expect, type Mock } from "vitest";

const THIS_PC = "this-computer";

type Impl = (cmd: string, args?: Record<string, unknown>) => unknown;

function unwrapped(impl: Impl): Impl {
  return (cmd, args) => {
    if (cmd === "remote_call" && args?.master === THIS_PC) {
      const inner = { ...((args.args as Record<string, unknown>) ?? {}) };
      if (args.streams) inner.onEvent = args.onEvent;
      return impl(args.cmd as string, inner);
    }
    return impl(cmd, args);
  };
}

/** The `invoke` arguments of one call to This PC's master, for `toHaveBeenCalledWith(...onThisPc(cmd, args))`. */
export function onThisPc(cmd: string, args: Record<string, unknown>): [string, unknown] {
  return ["remote_call", expect.objectContaining({ master: THIS_PC, cmd, args })];
}

/** From now on, the implementations given to this `invoke` mock see This PC's calls as plain commands. */
export function thisPcAsPlainCommands(invoke: unknown): void {
  const m = invoke as Mock<Impl>;
  const set = m.mockImplementation.bind(m);
  const once = m.mockImplementationOnce.bind(m);
  m.mockImplementation = ((impl: Impl) => set(unwrapped(impl))) as typeof m.mockImplementation;
  m.mockImplementationOnce = ((impl: Impl) => once(unwrapped(impl))) as typeof m.mockImplementationOnce;
}
