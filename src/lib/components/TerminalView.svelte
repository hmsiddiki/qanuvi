<script lang="ts">
  /** One terminal: an xterm.js screen connected to a real shell in the Rust core. */
  import { onDestroy, onMount } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import "@xterm/xterm/css/xterm.css";
  import { api, errorText } from "$lib/api";

  interface Props {
    projectId: string;
    shell: string;
    visible: boolean;
    onexit?: () => void;
    ontitle?: (title: string) => void;
  }

  let { projectId, shell, visible, onexit, ontitle }: Props = $props();

  let host: HTMLDivElement;
  let term: Terminal | null = null;
  let fit: FitAddon | null = null;
  let id: string | null = null;
  let observer: ResizeObserver | null = null;
  let exited = false;

  function resize() {
    if (!term || !fit || !host || host.offsetWidth === 0 || host.offsetHeight === 0) return;
    try {
      fit.fit();
    } catch {
      return;
    }
    if (id) api.term.resize(id, term.cols, term.rows).catch(() => {});
  }

  onMount(() => {
    term = new Terminal({
      fontFamily: getComputedStyle(document.body).getPropertyValue("--qx-font-mono") || "monospace",
      fontSize: 12,
      lineHeight: 1.25,
      cursorBlink: true,
      scrollback: 5000,
      allowProposedApi: false,
      theme: {
        background: "#141416",
        foreground: "#e4e4e8",
        cursor: "#4f8cff",
        selectionBackground: "#2d4370",
        black: "#1c1c1f",
        brightBlack: "#6b6b73",
        red: "#e5604b",
        brightRed: "#f07f7f",
        green: "#4cc38a",
        brightGreen: "#9ed38a",
        yellow: "#e5b84b",
        brightYellow: "#f2d27b",
        blue: "#4f8cff",
        brightBlue: "#7fb4ff",
        magenta: "#c79bf2",
        brightMagenta: "#dcbaf7",
        cyan: "#5fd4c4",
        brightCyan: "#8ee6da",
        white: "#c8c8cf",
        brightWhite: "#ffffff",
      },
    });
    fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host);
    resize();

    term.onData((data) => {
      if (id && !exited) api.term.write(id, data).catch(() => {});
    });
    term.onTitleChange((title) => ontitle?.(title));
    // Ctrl+C copies when text is selected; Ctrl+V pastes (the shell gets them otherwise).
    term.attachCustomKeyEventHandler((e) => {
      if (e.type !== "keydown" || !e.ctrlKey) return true;
      if (e.key === "c" && term?.hasSelection()) {
        navigator.clipboard.writeText(term.getSelection()).catch(() => {});
        return false;
      }
      if (e.key === "v") {
        navigator.clipboard
          .readText()
          .then((text) => {
            if (id) api.term.write(id, text);
          })
          .catch(() => {});
        return false;
      }
      return true;
    });

    api.term
      .spawn(projectId, shell, term.cols, term.rows, (event) => {
        if (event.kind === "data") term?.write(event.data);
        else {
          exited = true;
          term?.write("\r\n\x1b[90m[The shell has ended. Close this tab or open a new terminal.]\x1b[0m\r\n");
          onexit?.();
        }
      })
      .then((newId) => (id = newId))
      .catch((e) => term?.write(`\x1b[31m${errorText(e)}\x1b[0m\r\n`));

    observer = new ResizeObserver(() => resize());
    observer.observe(host);
  });

  $effect(() => {
    if (visible) {
      requestAnimationFrame(() => {
        resize();
        term?.focus();
      });
    }
  });

  onDestroy(() => {
    observer?.disconnect();
    if (id) api.term.kill(id).catch(() => {});
    term?.dispose();
  });
</script>

<div class="terminal-view" class:hidden={!visible} bind:this={host}></div>

<style>
  .terminal-view {
    position: absolute;
    inset: 0;
    padding: 4px 0 0 10px;
  }

  .hidden {
    visibility: hidden;
    pointer-events: none;
  }

  .terminal-view :global(.xterm-viewport) {
    background-color: transparent !important;
  }
</style>
