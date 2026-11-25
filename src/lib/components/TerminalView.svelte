<script lang="ts">
  /** One terminal: an xterm.js screen connected to a real shell in the Rust core. */
  import { clipboardText, contextMenu, copyText } from "$lib/stores/contextMenu.svelte";
  import { onDestroy, onMount } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import "@xterm/xterm/css/xterm.css";
  import { api, errorText, type TermEvent } from "$lib/api";
  import { terminals, type TerminalTab } from "$lib/stores/terminals.svelte";
  import { linkEndedText, linkLostText, onLinkBack, onLinkEnd, own, shownMachine } from "$lib/remote";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { EarlyInput, opensLink, osc52Copy, pasteText, urlsIn } from "$lib/terminalText";

  interface Props {
    projectId: string;
    shell: string;
    visible: boolean;
    /** The program ended; its exit code when known (a program run by `start`). */
    onexit?: (code: number | null) => void;
    ontitle?: (title: string) => void;
    /** A folder of the project to start in. */
    dir?: string;
    /** Starts something else than a shell (24z3 D2: a setup install); answers the terminal's id. */
    start?: (cols: number, rows: number, onEvent: (e: TermEvent) => void) => Promise<string>;
    /** Said when it ends (the shell's line otherwise). */
    endText?: string;
    /**
     * Its terminal tab (24z3 Part B2): a terminal started before is attached again (its recent output first), and a
     * screen that goes because its machine is hidden leaves the shell running.
     */
    tab?: TerminalTab;
  }

  let { projectId, shell, visible, onexit, ontitle, dir, start, endText, tab }: Props = $props();

  function menu(e: MouseEvent) {
    const t = term;
    if (!t) return;
    const selected = t.getSelection();
    contextMenu.show(e, [
      [
        { label: "Copy", disabled: !selected, action: () => copyText(selected) },
        {
          label: "Paste",
          disabled: !id || exited,
          action: async () => {
            const text = await clipboardText();
            // As typed by xterm (z5): a shell that asks for bracketed paste gets it, so a line break in the
            // text does not run the line; the owner edits it and presses Enter.
            if (text && id) t.paste(pasteText(text));
            t.focus();
          },
        },
        { label: "Select all", action: () => t.selectAll() },
      ],
      [{ label: "Clear", action: () => (t.clear(), t.focus()) }],
    ]);
  }

  let host: HTMLDivElement;
  let term: Terminal | null = null;
  let fit: FitAddon | null = null;
  let id: string | null = null;
  let observer: ResizeObserver | null = null;
  let exited = false;
  /** What the screen sends before the shell's id is known (v3): held, then sent once it is. */
  const early = new EarlyInput();
  /** Old output is being played back into a new screen (24z3 Part B2). */
  let replaying = false;
  let played = false;
  let stopListening: (() => void) | null = null;
  let gone = false;
  /** Its server's connection went; the shell goes on there and is taken over when it is back (c2). */
  let waiting = false;
  let stopBack: (() => void) | null = null;

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
      // Links a program prints as such (OSC 8, Claude Code's sign-in link): Ctrl+click opens them in the owner's
      // browser (z6); a selection or right-click that ends over one does nothing.
      linkHandler: {
        activate: (e, uri) => {
          if (opensLink(e)) void openUrl(uri);
        },
        hover: (_e, uri) => (host.title = `Ctrl+click to open ${uri}`),
        leave: () => (host.title = ""),
        allowNonHttpProtocols: false,
      },
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
    // Plain addresses in the output, one screen line each: the same Ctrl+click (z6).
    term.registerLinkProvider({
      provideLinks: (y, callback) => {
        const line = term?.buffer.active.getLine(y - 1)?.translateToString(true) ?? "";
        const links = urlsIn(line).map((u) => ({
          range: { start: { x: u.start + 1, y }, end: { x: u.end, y } },
          text: u.url,
          activate: (e: MouseEvent) => {
            if (opensLink(e)) void openUrl(u.url);
          },
          hover: () => (host.title = `Ctrl+click to open ${u.url}`),
          leave: () => (host.title = ""),
        }));
        callback(links.length ? links : undefined);
      },
    });
    // A program's "copy this" (OSC 52; Claude Code's "c to copy" on a server): onto this PC's clipboard (z7).
    // Reading the clipboard is never answered.
    term.parser.registerOscHandler(52, (data) => {
      const text = osc52Copy(data);
      if (text !== null) void copyText(text);
      return true;
    });

    term.onData((data) => {
      // Answers the screen gives to old output played back (a cursor question the shell asked long ago) are not
      // typed into the shell.
      if (replaying) return;
      // Nothing typed goes to a shell whose connection is gone (it would be lost); it is back with the connection.
      if (waiting || exited || early.hold(id, data)) return;
      api.term.write(id!, data).catch(() => {});
    });
    term.onTitleChange((title) => ontitle?.(title));
    // Every paste (Ctrl+V, Shift+Insert, the browser's own) goes through here once, before xterm's own handler.
    host.addEventListener(
      "paste",
      (e) => {
        e.preventDefault();
        e.stopPropagation();
        const text = e.clipboardData?.getData("text/plain") ?? "";
        if (text && !exited) term?.paste(pasteText(text));
      },
      true,
    );
    // Ctrl+C copies when text is selected; Ctrl+V pastes (the shell gets them otherwise).
    term.attachCustomKeyEventHandler((e) => {
      if (e.type !== "keydown" || !e.ctrlKey) return true;
      if (e.key === "c" && term?.hasSelection()) {
        navigator.clipboard.writeText(term.getSelection()).catch(() => {});
        return false;
      }
      // The paste itself comes as the paste event below (one paste, not two); the shell does not get ^V.
      if (e.key === "v") return false;
      return true;
    });

    const ended = (code: number | null, text: string) => {
      if (exited) return;
      exited = true;
      term?.write(`\r\n\x1b[90m[${text}]\x1b[0m\r\n`);
      onexit?.(code);
    };
    const onEvent = (event: TermEvent) => {
      if (event.kind === "data") {
        if (replaying) {
          played = true;
          term?.write(event.data, () => (replaying = false));
        } else term?.write(event.data);
      } else ended(event.code, endText ?? "The shell has ended. Close this tab or open a new terminal.");
    };
    // j1: the link its shell runs through ended. This PC's own master stopped: the shell ended with it, and no end
    // comes on its stream, so the screen says so. A server's connection went (c2: a Wi-Fi drop, a hotspot): its
    // master keeps the shell for a while, and this screen takes it over again when the machine is back.
    const machine = tab ? tab.machine : shownMachine();
    const takeBack = async (on: string) => {
      if (!waiting || !id || gone || exited) return;
      stopBack?.();
      stopBack = null;
      own(id, on);
      // The whole kept output comes again: the screen starts over with it.
      replaying = true;
      played = false;
      term?.reset();
      try {
        await api.term.attach(id, onEvent);
        waiting = false;
        if (!played) setTimeout(() => !played && (replaying = false), 500);
        resize();
      } catch {
        replaying = false;
        waiting = false;
        ended(null, linkEndedText(on));
      }
    };
    void onLinkEnd(machine, () => {
      if (!id || exited) return;
      if (machine === null) return ended(null, linkEndedText(machine));
      if (waiting) return;
      waiting = true;
      term?.write(`\r\n\x1b[90m[${linkLostText(machine)}]\x1b[0m\r\n`);
      stopBack = onLinkBack(machine, () => void takeBack(machine));
    }).then((stop) => {
      if (gone) stop();
      else stopListening = stop;
    });
    const again = tab?.termId;
    if (again) {
      // Back from a hidden machine: the same shell, with what it wrote meanwhile.
      id = again;
      replaying = true;
      api.term
        .attach(again, onEvent)
        .then(() => {
          if (tab) terminals.attached(tab.key);
          // Nothing kept to play back: the screen answers the shell again.
          if (!played) setTimeout(() => !played && (replaying = false), 500);
          resize();
        })
        .catch(() => {
          replaying = false;
          if (tab) terminals.attached(tab.key);
          ended(null, "This terminal ended while it was not shown. Close this tab or open a new terminal.");
        });
    } else
      (start
        ? start(term.cols, term.rows, onEvent)
        : api.term.spawn(projectId, shell, term.cols, term.rows, onEvent, dir ?? null)
      )
        .then((newId) => {
          // Closed while it was starting: nothing will stop it later.
          if (gone && (!tab || terminals.wasClosed(tab.key))) {
            api.term.kill(newId).catch(() => {});
            return;
          }
          id = newId;
          if (tab) terminals.started(tab.key, newId);
          // Typed, pasted or answered while it was starting: sent now, in order.
          const held = early.take();
          if (held && !exited) api.term.write(newId, held).catch(() => {});
        })
        .catch((e) => {
          early.take();
          term?.write(`\x1b[31m${errorText(e)}\x1b[0m\r\n`);
        });

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
    gone = true;
    stopListening?.();
    stopBack?.();
    // Only a closed tab (or the window going) stops the shell. A screen that goes while its tab stays (its machine
    // hidden, the terminal panel or work pane closed) leaves it running, its output kept for the next screen.
    // Whether the tab was closed is asked of the store's plain record: state read here is as it was before the change
    // that removed this screen, so the closed tab still looked listed and its shell was parked instead of stopped.
    if (id && !tab?.parked) {
      if (tab && !terminals.wasClosed(tab.key) && terminals.tabs.some((t) => t.key === tab.key))
        terminals.parkOne(tab.key);
      else api.term.kill(id).catch(() => {});
    }
    term?.dispose();
  });
</script>

<div class="terminal-view" class:hidden={!visible} bind:this={host} oncontextmenu={menu} role="presentation"></div>

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
