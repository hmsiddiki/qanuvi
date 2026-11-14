/**
 * Debugging in the editor (15i part 3): a breakpoint gutter (click the strip left of a line number, or the line
 * number itself, to add or take away a breakpoint), breakpoints that move with edits, and the paused line.
 */
import { RangeSet, StateEffect, StateField, type Extension } from "@codemirror/state";
import { Decoration, EditorView, GutterMarker, gutter, lineNumbers, type DecorationSet } from "@codemirror/view";

export interface BreakpointMark {
  line: number;
  verified: boolean;
  enabled: boolean;
}

class BreakpointMarker extends GutterMarker {
  constructor(readonly mark: BreakpointMark) {
    super();
  }
  eq(other: GutterMarker): boolean {
    return (
      other instanceof BreakpointMarker &&
      other.mark.verified === this.mark.verified &&
      other.mark.enabled === this.mark.enabled
    );
  }
  toDOM(): Node {
    const el = document.createElement("span");
    el.className = `qx-bp${this.mark.enabled ? "" : " off"}${this.mark.verified ? "" : " unbound"}`;
    el.title = !this.mark.enabled
      ? "Breakpoint (turned off)"
      : this.mark.verified
        ? "Breakpoint (click to remove)"
        : "Breakpoint the debugger could not bind here (no code on this line, or the file is not loaded)";
    return el;
  }
}

class PausedMarker extends GutterMarker {
  toDOM(): Node {
    const el = document.createElement("span");
    el.className = "qx-paused";
    el.title = "Paused here";
    return el;
  }
}

const pausedMarker = new PausedMarker();

/** The faint dot shown in the strip while the mouse is over that line's number. */
class HoverMarker extends GutterMarker {
  toDOM(): Node {
    const el = document.createElement("span");
    el.className = "qx-bp-hover";
    return el;
  }
}

const hoverMarker = new HoverMarker();

/** The line start under the mouse in the line numbers, or null. */
const setHoverLine = StateEffect.define<number | null>();

const hoverField = StateField.define<number | null>({
  create: () => null,
  update(pos, tr) {
    for (const e of tr.effects) if (e.is(setHoverLine)) return e.value;
    return pos === null || !tr.docChanged ? pos : null;
  },
});

/** Replaces the shown breakpoints (from the store). */
export const setBreakpointMarks = StateEffect.define<BreakpointMark[]>();
/** The paused line (1-based), or null. */
export const setPausedLine = StateEffect.define<number | null>();

const breakpointField = StateField.define<RangeSet<BreakpointMarker>>({
  create: () => RangeSet.empty,
  update(set, tr) {
    let next = set.map(tr.changes);
    for (const e of tr.effects) {
      if (e.is(setBreakpointMarks)) {
        const doc = tr.state.doc;
        const ranges = e.value
          .filter((m) => m.line >= 1 && m.line <= doc.lines)
          .sort((a, b) => a.line - b.line)
          .map((m) => new BreakpointMarker(m).range(doc.line(m.line).from));
        next = RangeSet.of(ranges, true);
      }
    }
    return next;
  },
});

const pausedField = StateField.define<{ pos: number | null; deco: DecorationSet }>({
  create: () => ({ pos: null, deco: Decoration.none }),
  update(value, tr) {
    let pos = value.pos === null ? null : tr.changes.mapPos(value.pos);
    for (const e of tr.effects) {
      if (e.is(setPausedLine)) {
        const n = e.value;
        pos = n !== null && n >= 1 && n <= tr.state.doc.lines ? tr.state.doc.line(n).from : null;
      }
    }
    if (pos === value.pos && !tr.docChanged) return value;
    const deco =
      pos === null ? Decoration.none : Decoration.set([Decoration.line({ class: "qx-paused-line" }).range(pos)]);
    return { pos, deco };
  },
  provide: (f) => EditorView.decorations.from(f, (v) => v.deco),
});

/** The breakpoint lines in the editor now (after edits moved them). */
export function breakpointLines(view: EditorView): number[] {
  const out: number[] = [];
  const iter = view.state.field(breakpointField).iter();
  while (iter.value) {
    out.push(view.state.doc.lineAt(iter.from).number);
    iter.next();
  }
  return out;
}

/**
 * The debugging extensions. `toggle` adds or takes away a breakpoint at a line; `moved` hears the lines after an
 * edit moved them.
 */
export function debugExtensions(toggle: (line: number) => void, moved: (lines: number[]) => void): Extension {
  return [
    breakpointField,
    pausedField,
    hoverField,
    gutter({
      class: "qx-bp-gutter",
      markers: (v) => v.state.field(breakpointField),
      lineMarker: (v, line) => {
        const p = v.state.field(pausedField).pos;
        if (p !== null && p >= line.from && p <= line.to) return pausedMarker;
        if (v.state.field(hoverField) !== line.from) return null;
        let has = false;
        v.state.field(breakpointField).between(line.from, line.from, () => {
          has = true;
          return false;
        });
        return has ? null : hoverMarker;
      },
      lineMarkerChange: (u) =>
        u.docChanged ||
        u.transactions.some((t) =>
          t.effects.some((e) => e.is(setPausedLine) || e.is(setHoverLine) || e.is(setBreakpointMarks)),
        ),
      initialSpacer: () => pausedMarker,
      domEventHandlers: {
        mousedown(view, line) {
          toggle(view.state.doc.lineAt(line.from).number);
          return true;
        },
      },
    }),
    // A click on the line number itself also adds or takes away the breakpoint (the strip is narrow). This joins
    // basicSetup's line numbers: their handlers are combined, the gutter is not added twice.
    lineNumbers({
      domEventHandlers: {
        mousedown(view, line, event) {
          if ((event as MouseEvent).button !== 0) return false;
          toggle(view.state.doc.lineAt(line.from).number);
          return true;
        },
        mousemove(view, line) {
          if (view.state.field(hoverField) !== line.from) view.dispatch({ effects: setHoverLine.of(line.from) });
          return false;
        },
        mouseleave(view) {
          if (view.state.field(hoverField) !== null) view.dispatch({ effects: setHoverLine.of(null) });
          return false;
        },
      },
    }),
    EditorView.updateListener.of((u) => {
      if (u.docChanged && u.state.field(breakpointField).size > 0) moved(breakpointLines(u.view));
    }),
    EditorView.baseTheme({
      ".qx-bp-gutter": { cursor: "pointer", minWidth: "14px" },
      ".qx-bp-gutter .cm-gutterElement": { display: "flex", alignItems: "center", justifyContent: "center" },
      ".qx-bp-gutter .cm-gutterElement:hover:empty::before": {
        content: '""',
        width: "9px",
        height: "9px",
        borderRadius: "50%",
        background: "#e5534b55",
      },
      ".cm-lineNumbers .cm-gutterElement": { cursor: "pointer" },
      ".qx-bp-hover": { width: "9px", height: "9px", borderRadius: "50%", background: "#e5534b55" },
      ".qx-bp": { width: "9px", height: "9px", borderRadius: "50%", background: "#e5534b" },
      ".qx-bp.unbound": { background: "transparent", border: "1.5px solid #9a9a9a", boxSizing: "border-box" },
      ".qx-bp.off": { background: "#9a9a9a" },
      ".qx-paused": {
        width: 0,
        height: 0,
        borderTop: "5px solid transparent",
        borderBottom: "5px solid transparent",
        borderLeft: "8px solid #e2b93d",
      },
      ".qx-paused-line": { backgroundColor: "#e2b93d26" },
    }),
  ];
}
