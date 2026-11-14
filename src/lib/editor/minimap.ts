/**
 * The minimap (15i part 4): a small picture of the file beside the editor, as in VS Code. It is painted on one
 * canvas, two pixels per line, in the syntax colours; only the lines that fit are painted, so a big file costs no
 * more than a small one. It shows where the editor is (the slider), the selection, search hits, git changes, and on
 * its right edge the errors and warnings of the whole file. Click or drag to scroll.
 */
import { syntaxTree } from "@codemirror/language";
import { forEachDiagnostic } from "@codemirror/lint";
import { getSearchQuery, searchPanelOpen } from "@codemirror/search";
import { EditorView } from "@codemirror/view";
import { highlightTree } from "@lezer/highlight";
import { gitMarks } from "$lib/git/gutter";
import { minimapHighlighter, minimapText } from "./theme";

export const MINIMAP_WIDTH = 90;
const LINE = 2;
const CHAR = 1;
const TAB = 4;
/** Left edge for git marks, right edge for the whole file's problems. */
const GIT_W = 2;
const RULER_W = 3;
const LEFT = GIT_W + 2;
const MAX_COLS = MINIMAP_WIDTH - LEFT - RULER_W - 2;
const MAX_HITS = 2000;

const GIT_COLORS = { added: "#2ea043", changed: "#3794ff", removed: "#f85149" } as const;
const PROBLEM_COLORS = { error: "#e5604b", warning: "#e5b84b" } as const;

interface Geometry {
  first: number;
  last: number;
  offset: number;
  contentH: number;
  sliderTop: number;
  sliderH: number;
  maxScroll: number;
}

export class Minimap {
  #frame = 0;
  #drag: { y: number; scroll: number } | null = null;
  #hover = false;
  #resize: ResizeObserver;
  #onScroll = () => this.schedule();

  constructor(
    readonly canvas: HTMLCanvasElement,
    readonly view: EditorView,
  ) {
    view.scrollDOM.addEventListener("scroll", this.#onScroll, { passive: true });
    this.#resize = new ResizeObserver(() => this.schedule());
    this.#resize.observe(canvas);
    canvas.addEventListener("pointerdown", this.#down);
    canvas.addEventListener("pointermove", this.#move);
    canvas.addEventListener("pointerup", this.#up);
    canvas.addEventListener("pointercancel", this.#up);
    canvas.addEventListener("pointerleave", this.#leave);
    canvas.addEventListener("wheel", this.#wheel, { passive: true });
    this.schedule();
  }

  /** Paints again at the next frame (many changes in one frame paint once). */
  schedule(): void {
    if (this.#frame) return;
    this.#frame = requestAnimationFrame(() => {
      this.#frame = 0;
      this.draw();
    });
  }

  destroy(): void {
    cancelAnimationFrame(this.#frame);
    this.view.scrollDOM.removeEventListener("scroll", this.#onScroll);
    this.#resize.disconnect();
    this.canvas.removeEventListener("pointerdown", this.#down);
    this.canvas.removeEventListener("pointermove", this.#move);
    this.canvas.removeEventListener("pointerup", this.#up);
    this.canvas.removeEventListener("pointercancel", this.#up);
    this.canvas.removeEventListener("pointerleave", this.#leave);
    this.canvas.removeEventListener("wheel", this.#wheel);
  }

  #geometry(): Geometry {
    const H = this.canvas.clientHeight;
    const sd = this.view.scrollDOM;
    const lines = this.view.state.doc.lines;
    const total = lines * LINE;
    const contentH = Math.min(total, H);
    const maxScroll = Math.max(0, sd.scrollHeight - sd.clientHeight);
    const ratio = maxScroll > 0 ? Math.min(1, sd.scrollTop / maxScroll) : 0;
    const shownLines = sd.clientHeight / Math.max(1, this.view.defaultLineHeight);
    const sliderH = Math.min(contentH, Math.max(6, shownLines * LINE));
    const startPx = total > H ? ratio * (total - H) : 0;
    const first = Math.floor(startPx / LINE) + 1;
    return {
      first,
      last: Math.min(lines, first + Math.ceil(H / LINE) + 1),
      offset: startPx - (first - 1) * LINE,
      contentH,
      sliderTop: ratio * Math.max(0, contentH - sliderH),
      sliderH,
      maxScroll,
    };
  }

  draw(): void {
    const canvas = this.canvas;
    const W = canvas.clientWidth;
    const H = canvas.clientHeight;
    if (!W || !H) return;
    const dpr = window.devicePixelRatio || 1;
    if (canvas.width !== Math.round(W * dpr) || canvas.height !== Math.round(H * dpr)) {
      canvas.width = Math.round(W * dpr);
      canvas.height = Math.round(H * dpr);
    }
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, W, H);

    const state = this.view.state;
    const doc = state.doc;
    const g = this.#geometry();
    const from = doc.line(g.first).from;
    const to = doc.line(g.last).to;
    const y = (line: number) => (line - g.first) * LINE - g.offset;
    const colOf = (lineText: string, ch: number) => {
      let col = 0;
      for (let i = 0; i < ch && col < MAX_COLS; i++) col += lineText.charCodeAt(i) === 9 ? TAB : 1;
      return col;
    };

    // Selection, behind the text.
    ctx.fillStyle = "#2d437099";
    for (const r of state.selection.ranges) {
      if (r.empty || r.to < from || r.from > to) continue;
      const a = Math.max(g.first, doc.lineAt(r.from).number);
      const b = Math.min(g.last, doc.lineAt(r.to).number);
      ctx.fillRect(LEFT, y(a), W - LEFT - RULER_W, (b - a + 1) * LINE);
    }

    // Search hits while the search panel is open: in place, and on the right edge for the whole file.
    const hitLines: number[] = [];
    const query = getSearchQuery(state);
    if (searchPanelOpen(state) && query.search && query.valid) {
      ctx.fillStyle = "#e5b84bcc";
      const cursor = query.getCursor(state);
      let n = 0;
      for (let m = cursor.next(); !m.done && n < MAX_HITS; m = cursor.next(), n++) {
        const line = doc.lineAt(m.value.from);
        hitLines.push(line.number);
        if (line.number < g.first || line.number > g.last) continue;
        const x = colOf(line.text, m.value.from - line.from);
        const w = Math.max(2, colOf(line.text, m.value.to - line.from) - x);
        ctx.fillRect(LEFT + x * CHAR, y(line.number), w * CHAR, LINE);
      }
    }

    // The text: runs of non-blank characters in their syntax colour.
    const spans: { from: number; to: number; color: string }[] = [];
    highlightTree(
      syntaxTree(state),
      minimapHighlighter,
      (f, t, cls) => {
        const color = cls.split(" ").pop();
        if (color) spans.push({ from: f, to: t, color });
      },
      from,
      to,
    );
    ctx.globalAlpha = 0.7;
    let s = 0;
    for (let n = g.first; n <= g.last; n++) {
      const line = doc.line(n);
      const text = line.text;
      const top = y(n);
      let col = 0;
      let runStart = -1;
      let runColor = "";
      const flush = (end: number) => {
        if (runStart < 0) return;
        ctx.fillStyle = runColor;
        ctx.fillRect(LEFT + runStart * CHAR, top, (end - runStart) * CHAR, LINE * 0.75);
        runStart = -1;
      };
      for (let i = 0; i < text.length && col < MAX_COLS; i++) {
        const code = text.charCodeAt(i);
        if (code === 32 || code === 9) {
          flush(col);
          col += code === 9 ? TAB : 1;
          continue;
        }
        const pos = line.from + i;
        while (s < spans.length && spans[s].to <= pos) s++;
        const color = s < spans.length && spans[s].from <= pos ? spans[s].color : minimapText;
        if (runStart >= 0 && color !== runColor) flush(col);
        if (runStart < 0) {
          runStart = col;
          runColor = color;
        }
        col++;
      }
      flush(Math.min(col, MAX_COLS));
    }
    ctx.globalAlpha = 1;

    // Git changes on the left edge.
    gitMarks(state, from, to, (pos, kind) => {
      ctx.fillStyle = GIT_COLORS[kind];
      ctx.fillRect(0, y(doc.lineAt(pos).number), GIT_W, LINE);
    });

    // The whole file on the right edge: search hits, warnings, errors (errors drawn last, on top).
    const rulerH = Math.max(g.contentH, Math.min(H, doc.lines * LINE));
    const at = (line: number) => ((line - 1) / Math.max(1, doc.lines)) * rulerH;
    ctx.fillStyle = "#e5b84b99";
    for (const l of hitLines) ctx.fillRect(W - RULER_W, at(l), RULER_W, 2);
    const problems: { line: number; error: boolean }[] = [];
    forEachDiagnostic(state, (d, f) => {
      if (d.severity === "error" || d.severity === "warning")
        problems.push({ line: doc.lineAt(Math.min(f, doc.length)).number, error: d.severity === "error" });
    });
    for (const p of problems.sort((a, b) => Number(a.error) - Number(b.error))) {
      ctx.fillStyle = p.error ? PROBLEM_COLORS.error : PROBLEM_COLORS.warning;
      ctx.fillRect(W - RULER_W, at(p.line), RULER_W, 3);
    }

    // Where the editor is.
    if (g.maxScroll > 0 || this.#hover) {
      ctx.fillStyle = this.#drag ? "#ffffff2e" : this.#hover ? "#ffffff1f" : "#ffffff12";
      ctx.fillRect(0, g.sliderTop, W - RULER_W, g.sliderH);
    }
  }

  #down = (e: PointerEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    const g = this.#geometry();
    const y = e.offsetY;
    // Outside the slider: the clicked line comes to the middle of the editor, then dragging goes on from there.
    if (y < g.sliderTop || y > g.sliderTop + g.sliderH) {
      const doc = this.view.state.doc;
      const line = Math.min(doc.lines, Math.max(1, g.first + Math.floor((y + g.offset) / LINE)));
      this.view.dispatch({ effects: EditorView.scrollIntoView(doc.line(line).from, { y: "center" }) });
    }
    this.#drag = { y: e.clientY, scroll: -1 };
    this.canvas.setPointerCapture(e.pointerId);
    this.schedule();
  };

  #move = (e: PointerEvent) => {
    if (!this.#drag) {
      if (!this.#hover) {
        this.#hover = true;
        this.schedule();
      }
      return;
    }
    const sd = this.view.scrollDOM;
    // The scroll position is read on the first move, after the click's own scroll has been applied.
    if (this.#drag.scroll < 0) this.#drag = { y: e.clientY, scroll: sd.scrollTop };
    const g = this.#geometry();
    const room = Math.max(1, g.contentH - g.sliderH);
    sd.scrollTop = this.#drag.scroll + ((e.clientY - this.#drag.y) * g.maxScroll) / room;
  };

  #up = (e: PointerEvent) => {
    if (!this.#drag) return;
    this.#drag = null;
    if (this.canvas.hasPointerCapture(e.pointerId)) this.canvas.releasePointerCapture(e.pointerId);
    this.schedule();
  };

  #leave = () => {
    if (this.#drag) return;
    this.#hover = false;
    this.schedule();
  };

  #wheel = (e: WheelEvent) => {
    this.view.scrollDOM.scrollTop += e.deltaY;
  };
}
