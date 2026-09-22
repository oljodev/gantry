import { FitAddon } from '@xterm/addon-fit';
import { Terminal } from '@xterm/xterm';
import { useEffect, useRef, useState } from 'react';
import '@xterm/xterm/css/xterm.css';

import { EmptyState } from '@/components/gantry/EmptyState';
import { TerminalIcon } from '@phosphor-icons/react';
import { events } from '@/bindings';
import { commands, isTauri, unwrap } from '@/lib/ipc/client';
import { useUiStore } from '@/lib/stores/uiStore';

/**
 * A terminal in the right pane (16 §5): the user's own shell, on a real pty, in the session's
 * folder.
 *
 * It is not the shell connector and shares nothing with it. The connector runs one command line
 * for the model with its input closed so the call can be captured and repeated
 * (`docs/connectors/shell.md` D3); this has a keyboard attached, which is what `vim`, `top`, an
 * interactive rebase and a password prompt need. Nothing the model can call reaches in here.
 *
 * The pty outlives this component on purpose. A tab that is switched away from, a chat that is
 * changed, a window that is reloaded — all of them unmount this while the shell keeps running,
 * so opening is "attach, and redraw what I missed" rather than "start". Closing the *tab* is
 * what ends the shell.
 */
export function TerminalView({ id, chatId }: { id: string; chatId?: string }) {
  const host = useRef<HTMLDivElement>(null);
  const terminal = useRef<Terminal | null>(null);
  const theme = useUiStore((s) => s.theme);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauri() || !host.current) return;
    const term = new Terminal({
      allowProposedApi: true,
      cursorBlink: true,
      // The app's own mono face and the design system's code size (15 §4), so the terminal is
      // part of the window rather than a rectangle from somewhere else.
      fontFamily: mono(),
      fontSize: 12,
      lineHeight: 1.5,
      scrollback: 10_000,
      theme: themeColors(),
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host.current);
    fit.fit();
    terminal.current = term;

    let live = true;
    let attached = false;
    // Keystrokes before the pty is attached would be typed into nothing; the shell's own
    // prompt arrives within a frame or two of opening, and nobody types faster than that.
    const typed = term.onData((data) => {
      if (attached) void commands.writeTerminal(id, data);
    });

    // Output that arrives before the scrollback has been written waits for it. The listener is
    // registered first on purpose — anything printed between here and the answer below would
    // otherwise be lost — and writing it straight away would put the newest lines above the
    // older ones the scrollback carries.
    const waiting: string[] = [];
    const output = events.terminalOutput.listen((e) => {
      if (e.payload.id !== id) return;
      if (attached) term.write(e.payload.data);
      else waiting.push(e.payload.data);
    });
    const exited = events.terminalExited.listen((e) => {
      if (e.payload.id !== id) return;
      attached = false;
      // Said in the terminal itself, where the last thing that happened already is. Dim, so it
      // reads as the window speaking rather than as more output.
      const code = e.payload.code;
      term.write(`\r\n\x1b[2m[the shell exited${code === null ? '' : ` with ${code}`}]\x1b[0m\r\n`);
    });

    void unwrap(commands.openTerminal(id, chatId ?? null, term.cols, term.rows))
      .then((opened) => {
        if (!live) return;
        // What it printed before this tab existed, or before it was last closed. Written
        // before the listener can add to it, because the two are one stream.
        if (opened.scrollback) term.write(opened.scrollback);
        for (const data of waiting.splice(0)) term.write(data);
        attached = true;
        term.focus();
      })
      .catch((err: unknown) => {
        if (live) setError(err instanceof Error ? err.message : String(err));
      });

    // The pane is resizable, so this is not a rare event: a shell that is not told its size
    // wraps every line at eighty columns however wide the tab is.
    const resize = new ResizeObserver(() => {
      if (!live) return;
      fit.fit();
      if (attached) void commands.resizeTerminal(id, term.cols, term.rows);
    });
    resize.observe(host.current);

    return () => {
      live = false;
      terminal.current = null;
      resize.disconnect();
      typed.dispose();
      void output.then((f) => f());
      void exited.then((f) => f());
      term.dispose();
    };
  }, [id, chatId]);

  // Following the app's theme is the whole reason the colours are tokens: when the window
  // changes theme the terminal is redrawn from the same variables as everything else. The
  // media query is here too, because "system" changes without anything in the app changing.
  useEffect(() => {
    const apply = () => {
      if (terminal.current) terminal.current.options.theme = themeColors();
    };
    apply();
    const system = window.matchMedia('(prefers-color-scheme: dark)');
    system.addEventListener('change', apply);
    return () => system.removeEventListener('change', apply);
  }, [theme]);

  if (!isTauri()) {
    return (
      <EmptyState
        className="h-full"
        icon={<TerminalIcon />}
        title="The terminal needs the app"
        hint="A browser tab has no shell to attach to. Run Gantry itself to use this."
      />
    );
  }
  if (error) {
    return (
      <EmptyState
        className="h-full"
        icon={<TerminalIcon />}
        title="No terminal could be started"
        hint={error}
      />
    );
  }
  return <div ref={host} className="h-full w-full bg-inset p-2" />;
}

/** The app's mono face (15 §4), resolved: xterm measures the font itself and needs a stack,
    not a custom property. */
function mono(): string {
  return getComputedStyle(document.documentElement).getPropertyValue('--font-mono').trim();
}

/** The sixteen colours, the cursor and the selection, read from `tokens.css` (15 A24). */
function themeColors(): Record<string, string> {
  const style = getComputedStyle(document.documentElement);
  const read = (name: string) => style.getPropertyValue(`--term-${name}`).trim();
  return {
    foreground: read('fg'),
    background: read('bg'),
    cursor: read('cursor'),
    cursorAccent: read('bg'),
    selectionBackground: read('selection'),
    black: read('black'),
    red: read('red'),
    green: read('green'),
    yellow: read('yellow'),
    blue: read('blue'),
    magenta: read('magenta'),
    cyan: read('cyan'),
    white: read('white'),
    brightBlack: read('bright-black'),
    brightRed: read('bright-red'),
    brightGreen: read('bright-green'),
    brightYellow: read('bright-yellow'),
    brightBlue: read('bright-blue'),
    brightMagenta: read('bright-magenta'),
    brightCyan: read('bright-cyan'),
    brightWhite: read('bright-white'),
  };
}
