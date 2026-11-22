/**
 * How machines are told apart (24z3 machine names): their names and their colours. A name is shown exactly as the
 * owner typed it; two machines never share one (letter case aside), and "This PC" belongs to this computer. Each
 * paired machine has a colour of its own (This PC stays plain), so the machine on screen is never mistaken.
 */

/** The longest name, in characters. */
export const NAME_MAX = 40;

/** Names that mean this computer in Qanuvi's own words. */
const RESERVED = ["this pc", "this computer"];

/** Characters that cannot be seen: control, zero-width and other format characters, private and unassigned ones. */
const INVISIBLE = /[\p{Cc}\p{Cf}\p{Co}\p{Cn}\p{Zl}\p{Zp}]/u;
const INVISIBLE_ALL = /[\p{Cc}\p{Cf}\p{Co}\p{Cn}\p{Zl}\p{Zp}]/gu;

/** The form two names are compared in: trimmed, the same letters however they were typed, any case. */
export function nameKey(name: string): string {
  return name.trim().normalize("NFKC").toLowerCase();
}

const length = (s: string) => [...s].length;

/**
 * What is wrong with a name the owner typed, or "" when it can be used. `others` are the other machines' names;
 * `device` checks a name for this device as another computer shows it (no other names to clash with).
 */
export function nameProblem(name: string, others: string[] = [], device = false): string {
  const n = name.trim();
  if (!n) return "Give it a name.";
  if (INVISIBLE.test(n))
    return "The name has a character that cannot be seen (a control or zero-width one); type it again.";
  if (length(n) > NAME_MAX) return `The name has ${length(n)} characters; at most ${NAME_MAX}.`;
  if (device) return "";
  if (RESERVED.includes(nameKey(n))) return `"${n}" is what Qanuvi calls this computer; choose another name.`;
  const taken = others.find((o) => nameKey(o) === nameKey(n));
  if (taken !== undefined) return `Another machine is already named "${taken}".`;
  return "";
}

/**
 * A name made from something the machine says about itself (its computer name, the name it gives through the relay):
 * invisible characters dropped, trimmed, cut to the longest name, and numbered when another machine has it already
 * ("basevm378 (2)"). "" when nothing is left.
 */
export function uniqueName(from: string, others: string[]): string {
  const base = [...from.replace(INVISIBLE_ALL, "").trim()].slice(0, NAME_MAX).join("").trim();
  if (!base) return "";
  const free = (n: string) => !RESERVED.includes(nameKey(n)) && !others.some((o) => nameKey(o) === nameKey(n));
  if (free(base)) return base;
  for (let i = 2; ; i++) {
    const tail = ` (${i})`;
    const n =
      [...base]
        .slice(0, NAME_MAX - tail.length)
        .join("")
        .trim() + tail;
    if (free(n)) return n;
  }
}

/** The machines' colours: clear on the light and the dark theme, and apart from the status colours' meaning. */
export const COLORS = ["#3b82f6", "#a855f7", "#14b8a6", "#f97316", "#ec4899", "#84cc16", "#06b6d4", "#eab308"];

/** A colour for a new machine: the first one no other machine has, or the next in turn when all are used. */
export function pickColor(used: (string | undefined)[]): string {
  return COLORS.find((c) => !used.includes(c)) ?? COLORS[used.length % COLORS.length];
}

/** Beside a paired machine's name: how its link goes now (a direct link at home, else the relay). */
export function relayRoad(direct: boolean | undefined): string {
  return direct ? "directly on the home network" : "through the relay";
}
