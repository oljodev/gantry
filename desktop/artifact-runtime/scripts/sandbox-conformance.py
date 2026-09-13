#!/usr/bin/env python3
"""Runs the sandbox conformance cases (docs/plan/13 §5) in WebKitGTK, the engine the Linux app
renders artifacts in, and reports one verdict per rule.

Every case from `src/conformance/cases.json` is mounted on its own in a real sandboxed iframe —
`sandbox="allow-scripts"`, `referrerpolicy="no-referrer"`, `srcdoc` carrying the app's CSP — and
tries the thing the rule forbids. A case reports `blocked` by returning that string or throwing;
anything else is an escape. One case per document, because three of the verdicts are the
engine's rather than the page's: a modal dialog, a started download and a navigation request are
things only the embedder sees.

The remote URLs are under the reserved `.invalid` TLD, so no hole in the sandbox can make this
talk to anyone. That is also why a page-side denial is not enough on its own for a case that
names a CSP directive: a request that dies in the resolver looks the same from inside the page.
Those cases additionally require the document to have reported a `securitypolicyviolation` for
the directive, and require the engine to have started no network request at all.

Needs python3-gi and the WebKit2 4.1 typelib (the Tauri build dependencies). On this machine it
runs on the host, not in the editor's sandbox:

    host-spawn python3 desktop/artifact-runtime/scripts/sandbox-conformance.py

Options:
    --config PATH   a JSON object overriding `csp`, `sandbox_flags`, `referrer_policy`,
                    `only` (a list of case ids) and `include_hangs`. The Rust conformance test
                    writes one so the engine runs the very strings the app ships.
    --only ID       run one case (repeatable).
    --include-hangs also run the cases marked `hang`, which are expected to freeze the web
                    process; each gets its own hard deadline.
    --json PATH     write the machine-readable report here as well as to stdout.
    --quiet         only the final `GANTRY_CONFORMANCE <json>` line.
    --probe-engine  load the engine, print `GANTRY_CONFORMANCE_ENGINE ok` and exit; this is
                    how the test decides whether an engine run is possible here.

Exit code is 0 when every case run was blocked, 1 otherwise, 2 when the engine is unusable.
"""

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
CASES = HERE.parent / 'src' / 'conformance' / 'cases.json'
HARNESS = HERE.parent / 'src' / 'conformance' / 'harness.js'

# The app's own values (docs/plan/13 §5), used when no --config overrides them. The test passes
# the strings it reads out of the shipped source instead, so the engine runs what ships.
DEFAULT_CSP = (
    "default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval' blob:; "
    "style-src 'unsafe-inline'; img-src data: blob:; font-src data:; media-src data: blob:; "
    "connect-src 'none'; frame-src 'none'; object-src 'none'; form-action 'none'; base-uri 'none'"
)
DEFAULT_FLAGS = 'allow-scripts'
DEFAULT_REFERRER = 'no-referrer'

# How long one case may take before the embedder gives up on it, in milliseconds. The driver's
# own per-case budget is 3 s and it adds a settle delay, so this is comfortably above both.
CASE_DEADLINE_MS = 12_000
HANG_DEADLINE_MS = 20_000


def escape_script(text: str) -> str:
    """A closing tag inside an inline script would end it early."""
    return text.replace('</script', '<\\/script').replace('</SCRIPT', '<\\/SCRIPT')


def case_document(case: dict, remote: str, csp: str, harness_js: str) -> str:
    """One `html` artifact carrying one case.

    Mirrors `conformanceDocument` in `src/conformance/probe.ts`; the cases and the driver are
    shared, only this ten-line wrapper is written twice, once per language.
    """
    entry = json.dumps(case['id']), json.dumps(case.get('directive'))
    script = '\n'.join(
        [
            f'var REMOTE = {json.dumps(remote)};',
            harness_js,
            '__gantryConformance([{id:%s,directive:%s,run:async function(){%s\n}}],'
            % (entry[0], entry[1], case['attempt']),
            '  function (id, verdict, detail, violations) {',
            "    parent.postMessage({kind:'verdict',id:id,verdict:verdict,detail:detail,"
            'violations:violations}, "*");',
            '  },',
            "  function () { parent.postMessage({kind:'done'}, '*'); },",
            '  3000);',
        ]
    )
    return (
        '<!doctype html><html><head><meta charset="utf-8">'
        f'<meta http-equiv="Content-Security-Policy" content="{csp}">'
        '<title>Sandbox conformance</title></head><body>'
        f'<script>{escape_script(script)}</script></body></html>'
    )


def parent_document(doc: str, flags: str, referrer: str) -> str:
    """The app's side: one sandboxed frame, and the same three checks `acceptMessage` makes —
    the message must come from that frame, from an opaque origin, and be plain JSON."""
    script = f"""
window.__result = null;
window.__notes = [];
var frame = document.createElement('iframe');
frame.setAttribute('sandbox', {json.dumps(flags)});
frame.setAttribute('referrerpolicy', {json.dumps(referrer)});
frame.style.cssText = 'width:100%;height:320px;border:0;display:block';
window.addEventListener('message', function (e) {{
  if (e.source !== frame.contentWindow) {{ window.__notes.push('message from another source'); return; }}
  if (e.origin !== 'null') {{ window.__notes.push('origin was ' + e.origin); return; }}
  var m = e.data;
  if (!m || typeof m !== 'object' || typeof m.kind !== 'string') return;
  if (m.kind === 'verdict') window.__result = m;
}});
document.body.appendChild(frame);
frame.srcdoc = {json.dumps(doc)};
"""
    return (
        '<!doctype html><html><body style="margin:0;background:#1f1f23">'
        f'<script>{escape_script(script)}</script></body></html>'
    )


def load_config(argv: list[str]) -> dict:
    config: dict = {}
    if '--config' in argv:
        config = json.loads(Path(argv[argv.index('--config') + 1]).read_text(encoding='utf-8'))
    only = [argv[i + 1] for i, a in enumerate(argv) if a == '--only']
    if only:
        config['only'] = only
    if '--include-hangs' in argv:
        config['include_hangs'] = True
    config.setdefault('csp', DEFAULT_CSP)
    config.setdefault('sandbox_flags', DEFAULT_FLAGS)
    config.setdefault('referrer_policy', DEFAULT_REFERRER)
    config.setdefault('include_hangs', False)
    config.setdefault('only', None)
    return config


def main() -> int:
    argv = sys.argv[1:]
    quiet = '--quiet' in argv
    config = load_config(argv)

    try:
        import gi

        gi.require_version('Gtk', '3.0')
        gi.require_version('WebKit2', '4.1')
        from gi.repository import GLib, Gtk, WebKit2

        if not Gtk.init_check()[0]:
            raise ValueError('no display: GTK could not open one')
    except (ImportError, ValueError, AttributeError) as err:
        print(f'GANTRY_CONFORMANCE_UNAVAILABLE {err}', file=sys.stderr)
        return 2

    if '--probe-engine' in argv:
        print('GANTRY_CONFORMANCE_ENGINE ok')
        return 0

    spec = json.loads(CASES.read_text(encoding='utf-8'))
    harness_js = HARNESS.read_text(encoding='utf-8')
    remote = spec['remote']
    cases = [c for c in spec['cases'] if config['include_hangs'] or not c.get('hang')]
    if config['only']:
        cases = [c for c in cases if c['id'] in config['only']]
    if not cases:
        print('GANTRY_CONFORMANCE_UNAVAILABLE no cases selected', file=sys.stderr)
        return 2

    settings = WebKit2.Settings()
    settings.set_enable_write_console_messages_to_stdout(False)
    view = WebKit2.WebView(settings=settings)
    window = Gtk.Window()
    window.set_default_size(700, 420)
    window.add(view)
    window.show_all()

    # What only the embedder sees. Reset per case.
    seen: dict[str, list[str]] = {'navigation': [], 'dialog': [], 'download': [], 'resource': []}

    def on_dialog(_view, dialog):
        seen['dialog'].append(dialog.get_message() or 'dialog')
        return True  # handled: never actually show one, which would stall the run

    def on_policy(_view, decision, decision_type):
        if decision_type == WebKit2.PolicyDecisionType.NAVIGATION_ACTION:
            uri = decision.get_navigation_action().get_request().get_uri() or ''
            if 'gantry-sandbox.invalid' in uri:
                seen['navigation'].append(uri)
                decision.ignore()
                return True
        return False

    def on_download(_context, download):
        seen['download'].append(download.get_request().get_uri() or 'download')
        download.cancel()

    def on_resource(_view, resource, _request):
        uri = resource.get_uri() or ''
        if 'gantry-sandbox.invalid' in uri or 'ipc.localhost' in uri:
            seen['resource'].append(uri)

    view.connect('script-dialog', on_dialog)
    view.connect('decide-policy', on_policy)
    view.connect('resource-load-started', on_resource)
    view.get_context().connect('download-started', on_download)

    results: list[dict] = []
    loop = GLib.MainLoop()
    index = 0
    elapsed = 0

    def verdict_for(case: dict, page: dict | None) -> dict:
        rule = case.get('rule', '')
        if page is None:
            return {
                'id': case['id'],
                'rule': rule,
                'verdict': 'OPEN',
                'detail': f'no verdict within {CASE_DEADLINE_MS} ms',
                'engine': dict(seen),
            }
        out = {
            'id': case['id'],
            'rule': rule,
            'verdict': page.get('verdict', 'OPEN'),
            'detail': page.get('detail', ''),
            'violations': page.get('violations', []),
            'engine': {k: list(v) for k, v in seen.items()},
        }
        if out['verdict'] == 'blocked':
            observer = case.get('observer')
            if observer and seen.get(observer):
                out['verdict'] = 'OPEN'
                out['detail'] = f'the engine saw a {observer}: {seen[observer][0]}'
            elif case.get('directive') and seen['resource']:
                out['verdict'] = 'OPEN'
                out['detail'] = f'the engine started a request for {seen["resource"][0]}'
        return out

    def start_next():
        nonlocal index, elapsed
        for key in seen:
            seen[key] = []
        elapsed = 0
        case = cases[index]
        doc = case_document(case, remote, config['csp'], harness_js)
        view.load_html(parent_document(doc, config['sandbox_flags'], config['referrer_policy']),
                       'file:///gantry-conformance/')

    def finish_case(page: dict | None):
        nonlocal index
        case = cases[index]
        out = verdict_for(case, page)
        results.append(out)
        if not quiet:
            mark = 'ok  ' if out['verdict'] == 'blocked' else 'OPEN'
            print(f'{mark} {out["id"]:<20} {out["rule"]}', file=sys.stderr)
            if out['detail']:
                print(f'       {out["detail"]}', file=sys.stderr)
        index += 1
        if index >= len(cases):
            loop.quit()
        else:
            start_next()

    def poll():
        nonlocal elapsed
        if index >= len(cases):
            return False
        elapsed += 200
        deadline = HANG_DEADLINE_MS if cases[index].get('hang') else CASE_DEADLINE_MS

        def read(_view, result):
            try:
                raw = view.evaluate_javascript_finish(result).to_string()
                page = json.loads(raw) if raw and raw != 'null' else None
            except (GLib.Error, ValueError):
                page = None  # the page has not finished loading yet
            if page is not None:
                finish_case(page)
            elif elapsed >= deadline:
                finish_case(None)

        try:
            view.evaluate_javascript(
                'JSON.stringify(window.__result)', -1, None, None, None, read
            )
        except GLib.Error:
            if elapsed >= deadline:
                finish_case(None)
        return index < len(cases)

    start_next()
    GLib.timeout_add(200, poll)
    loop.run()

    report = {
        'csp': config['csp'],
        'sandbox_flags': config['sandbox_flags'],
        'referrer_policy': config['referrer_policy'],
        'cases': results,
    }
    line = 'GANTRY_CONFORMANCE ' + json.dumps(report)
    print(line)
    if '--json' in argv:
        Path(argv[argv.index('--json') + 1]).write_text(json.dumps(report, indent=2) + '\n')
    return 0 if all(r['verdict'] == 'blocked' for r in results) else 1


if __name__ == '__main__':
    sys.exit(main())
