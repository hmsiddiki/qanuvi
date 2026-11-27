<script lang="ts">
  import "$lib/styles/base.css";

  let { children } = $props();

  /**
   * m6 (Mac drive): macOS capitalised and corrected typed words in Qanuvi's boxes ("teal" became "Teal"), which
   * breaks names, paths and code. Like VS Code, no box changes what is typed; a box that wants it says so itself.
   */
  function plainTyping(e: Event) {
    const t = e.target;
    if (!(t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement)) return;
    if (!t.hasAttribute("autocapitalize")) t.setAttribute("autocapitalize", "off");
    if (!t.hasAttribute("autocorrect")) t.setAttribute("autocorrect", "off");
  }
</script>

<!-- Pointer-down comes before the focus, so the first click already types plainly. -->
<svelte:document onpointerdowncapture={plainTyping} onfocusin={plainTyping} />

{@render children()}
