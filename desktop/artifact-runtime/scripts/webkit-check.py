#!/usr/bin/env python3
"""Loads the built sandbox document in WebKitGTK, the engine Gantry runs in on Linux, mounts
one artifact of each executable type through the real bridge protocol (docs/plan/13 §5) and
prints every message that comes back. With `--png <path>` it also saves a screenshot of the
mounted frames, which is the only way to see that a diagram or a component actually drew.

Each case is also zoomed to 2x (13 §4) and the height it reports afterwards is required to be
twice the one before. That is the half of page zoom no unit test can check: `scrollHeight` does
not move when a document is zoomed, so the document has to multiply by the factor it was given,
and whether it did is a question only an engine answers.

Needs python3-gi and the WebKit2 4.1 typelib (the Tauri build dependencies), and the runtime
built (`pnpm runtime:build`). On this machine it runs on the host, not in the sandbox:

    host-spawn python3 desktop/artifact-runtime/scripts/webkit-check.py --png /tmp/check.png
"""

import json
import sys
from pathlib import Path

import gi

gi.require_version('Gtk', '3.0')
gi.require_version('WebKit2', '4.1')
from gi.repository import GLib, Gtk, WebKit2  # noqa: E402

RUNTIME = Path(__file__).resolve().parent.parent / 'dist' / 'runtime.html'

REACT = """
import React, { useState } from 'react';

export default function App() {
  const [n, setN] = useState(0);
  return (
    <div style={{ fontFamily: 'system-ui', padding: 12 }}>
      <h1>Counter {n}</h1>
      <button onClick={() => setN(n + 1)}>+</button>
    </div>
  );
}
"""

MERMAID = """graph TD
  A[Input] --> B[Hidden layer]
  B --> C[Output]
"""

# Mounted with the ground the app's prelude injects from its tokens (the prelude itself lives
# in the app, not in this package): a page that styles nothing must still read as a page, not
# as dark text on the panel's dark card.
HTML = """<!doctype html><html><head>
<style>html{color-scheme:light;background:#ffffff;color:#111114}</style></head>
<body style="font-family:system-ui;padding:12px">
<h1>Plain page</h1><p>With no colours of its own.</p></body></html>"""

PARENT = """<!doctype html><html><body style="margin:0;background:#1f1f23;color:#eee;font:13px system-ui">
<script>
window.__log = [];
const runtime = %(runtime)s;
const cases = %(cases)s;
function mount(c, i) {
  const nonce = 'n' + i;
  let natural = null, zoomed = null;
  const frame = document.createElement('iframe');
  frame.setAttribute('sandbox', 'allow-scripts');
  frame.style.cssText = 'width:100%%;height:220px;border:0;display:block;background:transparent';
  window.addEventListener('message', (e) => {
    if (e.source !== frame.contentWindow) return;
    const m = e.data || {};
    window.__log.push({case: c.type, origin: e.origin, kind: m.kind, nonceOk: m.nonce === nonce,
      message: m.message, phase: m.phase, level: m.level, text: m.text, height: m.height});
    if (m.kind === 'loaded') {
      frame.contentWindow.postMessage({kind: 'mount', nonce, type: c.type, content: c.content,
        theme: 'dark', language: c.language}, '*');
    }
    if (m.kind === 'resize' && m.height) {
      frame.style.height = Math.min(400, Math.max(80, m.height)) + 'px';
      // 13 §4: a zoom has to change the height the document reports, or the frame keeps its
      // old size around scaled-up content. Natural first, then the same document at 2x.
      if (natural === null) {
        natural = m.height;
        frame.contentWindow.postMessage({kind: 'zoom', nonce, factor: 2}, '*');
      } else if (zoomed === null) {
        zoomed = m.height;
        window.__log.push({case: c.type, kind: 'zoom', natural: natural, zoomed: zoomed,
          ok: Math.abs(zoomed - natural * 2) <= 2});
      }
    }
  });
  const label = document.createElement('div');
  label.textContent = c.type;
  label.style.cssText = 'padding:4px 8px;color:#a2a2ad';
  document.body.append(label, frame);
  frame.srcdoc = c.type === 'html' ? c.content : runtime;
}
cases.forEach(mount);
setTimeout(function () { window.__log.push({done: true}); }, 6000);
</script></body></html>"""


def main() -> int:
    runtime = RUNTIME.read_text(encoding='utf-8')
    cases = [
        {'type': 'react', 'content': REACT, 'language': 'tsx'},
        {'type': 'mermaid', 'content': MERMAID},
        {'type': 'html', 'content': HTML},
    ]
    # `</script>` inside an embedded document would end the parent's own script early.
    html = PARENT % {
        'runtime': json.dumps(runtime).replace('</', '<\\/'),
        'cases': json.dumps(cases).replace('</', '<\\/'),
    }

    png = sys.argv[sys.argv.index('--png') + 1] if '--png' in sys.argv else None

    settings = WebKit2.Settings()
    settings.set_enable_write_console_messages_to_stdout(True)
    settings.set_enable_developer_extras(True)
    view = WebKit2.WebView(settings=settings)
    window = Gtk.Window()
    window.set_default_size(900, 1000)
    window.add(view)
    window.show_all()

    seen = 0
    failures = 0
    loop = GLib.MainLoop()

    def finish():
        if not png:
            loop.quit()
            return

        def saved(_view, result):
            try:
                view.get_snapshot_finish(result).write_to_png(png)
                print('screenshot:', png)
            except Exception as err:  # noqa: BLE001
                print('screenshot failed:', err)
            loop.quit()

        view.get_snapshot(
            WebKit2.SnapshotRegion.FULL_DOCUMENT, WebKit2.SnapshotOptions.NONE, None, saved
        )

    def poll():
        nonlocal seen, failures

        def done(_view, result):
            nonlocal seen, failures
            try:
                entries = json.loads(view.evaluate_javascript_finish(result).to_string())
            except Exception:  # noqa: BLE001 - the page has not finished loading yet
                return
            for entry in entries[seen:]:
                print('bridge:', json.dumps(entry))
                if entry.get('kind') == 'error':
                    failures += 1
                if entry.get('kind') == 'zoom' and not entry.get('ok'):
                    failures += 1
                if entry.get('done'):
                    finish()
            seen = len(entries)

        view.evaluate_javascript('JSON.stringify(window.__log)', -1, None, None, None, done)
        return True

    GLib.timeout_add(400, poll)
    GLib.timeout_add(60_000, lambda: (print('gave up after 60 s'), loop.quit()) and False)
    view.load_html(html, 'file:///gantry-check/')
    loop.run()
    return 1 if failures else 0


if __name__ == '__main__':
    sys.exit(main())
