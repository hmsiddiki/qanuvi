/**
 * Change markers in the editor's gutter, as in VS Code (Milestone 15b): a green bar for added lines, a blue bar for
 * changed lines and a red mark where lines were removed, compared with the staged version of the file. They are worked
 * out again shortly after typing stops, so typing stays cheap in big files.
 */
import { RangeSet, RangeSetBuilder, StateEffect, StateField, type Text } from "@codemirror/state";
import { EditorView, GutterMarker, ViewPlugin, gutter, type ViewUpdate } from "@codemirror/view";
import { lineMarks, type Mark } from "./hunks";

/** Sets the text to compare with (null: no markers, for example a file git does not know yet). */
export const setGitBase = StateEffect.define<string | null>();
const recompute = StateEffect.define<null>();

const MAX_CHARS = 1_000_000;

class ChangeMarker extends GutterMarker {
  constructor(readonly kind: Mark) {
    super();
  }
  eq(other: ChangeMarker) {
    return other.kind === this.kind;
  }
  toDOM() {
    const el = document.createElement("div");
    el.className = `qx-git-${this.kind}`;
    return el;
  }
}

const MARKERS: Record<Mark, ChangeMarker> = {
  added: new ChangeMarker("added"),
  changed: new ChangeMarker("changed"),
  removed: new ChangeMarker("removed"),
};

function compute(base: string | null, doc: Text): RangeSet<GutterMarker> {
  if (base === null || doc.length > MAX_CHARS || base.length > MAX_CHARS) return RangeSet.empty;
  const b = new RangeSetBuilder<GutterMarker>();
  const marks = lineMarks(base, doc.toString());
  let last = 0;
  for (const n of [...marks.keys()].sort((x, y) => x - y)) {
    // A removal at the very end is marked on the last line.
    const line = Math.min(n, doc.lines);
    if (line <= last) continue;
    last = line;
    b.add(doc.line(line).from, doc.line(line).from, MARKERS[marks.get(n)!]);
  }
  return b.finish();
}

const baseField = StateField.define<string | null>({
  create: () => null,
  update(value, tr) {
    for (const e of tr.effects) if (e.is(setGitBase)) return e.value;
    return value;
  },
});

const marksField = StateField.define<RangeSet<GutterMarker>>({
  create: () => RangeSet.empty,
  update(value, tr) {
    if (tr.effects.some((e) => e.is(setGitBase) || e.is(recompute)))
      return compute(tr.state.field(baseField), tr.state.doc);
    return tr.docChanged ? value.map(tr.changes) : value;
  },
});

/** Works the markers out again 400 ms after the last change. */
const debounce = ViewPlugin.fromClass(
  class {
    timer: ReturnType<typeof setTimeout> | undefined;
    constructor(readonly view: EditorView) {}
    update(u: ViewUpdate) {
      if (!u.docChanged || u.state.field(baseField) === null) return;
      clearTimeout(this.timer);
      this.timer = setTimeout(() => this.view.dispatch({ effects: recompute.of(null) }), 400);
    }
    destroy() {
      clearTimeout(this.timer);
    }
  },
);

const theme = EditorView.baseTheme({
  ".qx-git-gutter": { width: "4px" },
  ".qx-git-gutter .cm-gutterElement": { padding: "0" },
  ".qx-git-added, .qx-git-changed": { width: "3px", height: "100%" },
  ".qx-git-added": { background: "#2ea043" },
  ".qx-git-changed": { background: "#3794ff" },
  ".qx-git-removed": {
    width: "0",
    height: "0",
    borderTop: "4px solid transparent",
    borderBottom: "4px solid transparent",
    borderLeft: "5px solid #f85149",
    marginTop: "-4px",
  },
});

export function gitGutter() {
  return [
    baseField,
    marksField,
    debounce,
    theme,
    gutter({ class: "qx-git-gutter", markers: (v) => v.state.field(marksField) }),
  ];
}
