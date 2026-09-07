/**
 * The sandbox conformance artifact (docs/plan/13 §5): an `html` artifact whose script tries
 * every way out of the sandbox and reports each outcome through the bridge as console lines
 * `probe <name>: blocked|OPEN <detail>`. The app asserts that every attempt is blocked.
 * Exported as a string so the gallery and the tests can mount it.
 */

export const CONFORMANCE_HTML = `<!doctype html>
<html><head><meta charset="utf-8"><title>Sandbox conformance</title></head>
<body style="font-family:ui-monospace,monospace;font-size:12px">
<h3>Sandbox conformance</h3><ul id="out"></ul>
<script>
(async () => {
  const out = document.getElementById('out');
  const say = (name, blocked, detail) => {
    const li = document.createElement('li');
    li.textContent = name + ': ' + (blocked ? 'blocked' : 'OPEN') + (detail ? ' (' + detail + ')' : '');
    out.appendChild(li);
    console.log('probe ' + name + ': ' + (blocked ? 'blocked' : 'OPEN') + (detail ? ' ' + detail : ''));
  };
  const attempt = async (name, fn) => {
    try {
      const r = await fn();
      say(name, r === 'blocked', r === 'blocked' ? '' : String(r));
    } catch (e) {
      say(name, true, (e && e.message) ? e.message.slice(0, 60) : String(e));
    }
  };
  await attempt('__TAURI_INTERNALS__', () => (window.__TAURI_INTERNALS__ ? 'present' : 'blocked'));
  await attempt('__TAURI__', () => (window.__TAURI__ ? 'present' : 'blocked'));
  await attempt('fetch ipc.localhost', async () => { await fetch('http://ipc.localhost/'); return 'fetched'; });
  await attempt('fetch https', async () => { await fetch('https://example.com'); return 'fetched'; });
  await attempt('WebSocket', () => new Promise((res, rej) => { try { const ws = new WebSocket('wss://example.com'); ws.onerror = () => rej(new Error('socket error')); ws.onopen = () => res('opened'); setTimeout(() => rej(new Error('timeout')), 1500); } catch (e) { rej(e); } }));
  await attempt('parent.document', () => (window.parent.document ? 'readable' : 'blocked'));
  await attempt('top.location', () => { window.top.location = 'https://example.com'; return 'navigated'; });
  await attempt('localStorage', () => { window.localStorage.setItem('x', '1'); return 'stored'; });
  await attempt('indexedDB', () => new Promise((res, rej) => { try { const r = indexedDB.open('x'); r.onerror = () => rej(new Error('open error')); r.onsuccess = () => res('opened'); } catch (e) { rej(e); } }));
  await attempt('clipboard.writeText', async () => { await navigator.clipboard.writeText('x'); return 'wrote'; });
  await attempt('window.open', () => (window.open('https://example.com') ? 'opened' : 'blocked'));
  await attempt('form submit', () => { const f = document.createElement('form'); f.action = 'https://example.com'; document.body.appendChild(f); f.submit(); return 'submitted'; });
  console.log('probe done');
})();
</script></body></html>`;

/** The names the app expects to see reported, all as `blocked`. */
export const PROBES = [
  '__TAURI_INTERNALS__',
  '__TAURI__',
  'fetch ipc.localhost',
  'fetch https',
  'WebSocket',
  'parent.document',
  'top.location',
  'localStorage',
  'indexedDB',
  'clipboard.writeText',
  'window.open',
  'form submit',
];
