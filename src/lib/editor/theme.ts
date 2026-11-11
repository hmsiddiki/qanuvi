/** Qanuvi's dark editor theme, built from the design tokens. */
import { EditorView } from "@codemirror/view";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";

const c = {
  bg: "#1c1c1f",
  gutter: "#1c1c1f",
  line: "#232327",
  selection: "#2d4370",
  text: "#e4e4e8",
  muted: "#6b6b73",
  secondary: "#9b9ba3",
  cursor: "#4f8cff",
  keyword: "#c79bf2",
  string: "#9ed38a",
  number: "#f2b56b",
  fn: "#7fb4ff",
  type: "#5fd4c4",
  comment: "#6b6b73",
  tag: "#f07f7f",
  attr: "#e5c07b",
  invalid: "#e5604b",
};

export const qanuviTheme = EditorView.theme(
  {
    "&": { color: c.text, backgroundColor: c.bg, height: "100%", fontSize: "var(--qx-editor-font-size, 13px)" },
    ".cm-scroller": { fontFamily: "var(--qx-font-mono)", lineHeight: "1.6" },
    ".cm-content": { caretColor: c.cursor, padding: "8px 0" },
    ".cm-cursor, .cm-dropCursor": { borderLeftColor: c.cursor, borderLeftWidth: "2px" },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
      backgroundColor: c.selection,
    },
    ".cm-activeLine": { backgroundColor: "#ffffff08" },
    ".cm-gutters": { backgroundColor: c.gutter, color: c.muted, border: "none" },
    ".cm-activeLineGutter": { backgroundColor: "transparent", color: c.secondary },
    ".cm-lineNumbers .cm-gutterElement": { padding: "0 12px 0 16px" },
    ".cm-foldPlaceholder": { backgroundColor: c.line, border: "none", color: c.secondary },
    ".cm-matchingBracket, .cm-nonmatchingBracket": { backgroundColor: "#4f8cff33", outline: "none" },
    ".cm-searchMatch": { backgroundColor: "#e5b84b44", outline: "1px solid #e5b84b66" },
    ".cm-searchMatch.cm-searchMatch-selected": { backgroundColor: "#e5b84b88" },
    ".cm-selectionMatch": { backgroundColor: "#ffffff14" },
    ".cm-panels": { backgroundColor: "#232327", color: c.text },
    ".cm-panels.cm-panels-top": { borderBottom: "1px solid #2c2c31" },
    ".cm-panels.cm-panels-bottom": { borderTop: "1px solid #2c2c31" },
    ".cm-panel input, .cm-panel button": { fontFamily: "inherit", fontSize: "12px" },
    ".cm-textfield": {
      backgroundColor: c.bg,
      border: "1px solid #2c2c31",
      borderRadius: "6px",
      color: c.text,
    },
    ".cm-button": {
      backgroundImage: "none",
      backgroundColor: "#2c2c31",
      border: "none",
      borderRadius: "6px",
      color: c.text,
    },
    ".cm-tooltip": { backgroundColor: "#232327", border: "1px solid #2c2c31", borderRadius: "8px" },
    ".cm-tooltip-autocomplete > ul > li[aria-selected]": { backgroundColor: "#2d4370", color: c.text },
  },
  { dark: true },
);

export const qanuviHighlight = syntaxHighlighting(
  HighlightStyle.define([
    { tag: [t.keyword, t.modifier, t.controlKeyword, t.operatorKeyword], color: c.keyword },
    { tag: [t.string, t.special(t.string), t.regexp], color: c.string },
    { tag: [t.number, t.bool, t.null, t.atom], color: c.number },
    { tag: [t.function(t.variableName), t.function(t.propertyName)], color: c.fn },
    { tag: [t.typeName, t.className, t.namespace], color: c.type },
    { tag: [t.comment, t.lineComment, t.blockComment, t.docComment], color: c.comment, fontStyle: "italic" },
    { tag: [t.tagName], color: c.tag },
    { tag: [t.attributeName, t.propertyName], color: c.attr },
    { tag: [t.variableName, t.self], color: c.text },
    { tag: [t.definition(t.variableName)], color: "#f2f2f5" },
    { tag: [t.operator, t.punctuation, t.bracket], color: c.secondary },
    { tag: [t.heading], color: c.fn, fontWeight: "600" },
    { tag: [t.link, t.url], color: c.fn, textDecoration: "underline" },
    { tag: [t.emphasis], fontStyle: "italic" },
    { tag: [t.strong], fontWeight: "600" },
    { tag: [t.meta, t.processingInstruction], color: c.secondary },
    { tag: t.invalid, color: c.invalid },
  ]),
);
