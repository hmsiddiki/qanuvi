<script lang="ts">
  /**
   * A drag handle between two panes. Reports the size the pane should take, measured from where the drag started,
   * so the pane follows the pointer exactly. Arrow keys move it too.
   */
  interface Props {
    /** "x" for a handle between side-by-side panes, "y" for one between stacked panes. */
    axis: "x" | "y";
    /** Current size of the pane this handle resizes. */
    size: number;
    /** +1 when moving the pointer right/down makes the pane bigger, -1 when it makes it smaller. */
    direction?: 1 | -1;
    label: string;
    onresize: (size: number) => void;
    ondone?: () => void;
  }

  let { axis, size, direction = 1, label, onresize, ondone }: Props = $props();

  let dragging = $state(false);
  let startPointer = 0;
  let startSize = 0;

  function pointerDown(event: PointerEvent) {
    if (event.button !== 0) return;
    dragging = true;
    startPointer = axis === "x" ? event.clientX : event.clientY;
    startSize = size;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    event.preventDefault();
  }

  function pointerMove(event: PointerEvent) {
    if (!dragging) return;
    const now = axis === "x" ? event.clientX : event.clientY;
    onresize(startSize + (now - startPointer) * direction);
  }

  function pointerUp(event: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    (event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
    ondone?.();
  }

  function keyDown(event: KeyboardEvent) {
    const step = event.shiftKey ? 40 : 10;
    const grow = axis === "x" ? "ArrowRight" : "ArrowDown";
    const shrink = axis === "x" ? "ArrowLeft" : "ArrowUp";
    if (event.key === grow) onresize(size + step * direction);
    else if (event.key === shrink) onresize(size - step * direction);
    else return;
    event.preventDefault();
    ondone?.();
  }
</script>

<!-- A focusable separator is the accessible pattern for a resize handle (WAI-ARIA "window splitter"). -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="splitter {axis}"
  class:dragging
  role="separator"
  aria-orientation={axis === "x" ? "vertical" : "horizontal"}
  aria-label={label}
  aria-valuenow={size}
  tabindex="0"
  onpointerdown={pointerDown}
  onpointermove={pointerMove}
  onpointerup={pointerUp}
  onpointercancel={pointerUp}
  onkeydown={keyDown}
></div>

<style>
  .splitter {
    position: relative;
    flex: none;
    z-index: 5;
    outline: none;
  }

  .splitter.x {
    width: 6px;
    margin: 0 -3px;
    cursor: col-resize;
  }

  .splitter.y {
    height: 6px;
    margin: -3px 0;
    cursor: row-resize;
  }

  .splitter::after {
    content: "";
    position: absolute;
    background: transparent;
    transition: background 120ms;
  }

  .splitter.x::after {
    inset: 0 2px;
  }

  .splitter.y::after {
    inset: 2px 0;
  }

  .splitter:hover::after,
  .splitter:focus-visible::after,
  .splitter.dragging::after {
    background: var(--qx-accent);
  }
</style>
