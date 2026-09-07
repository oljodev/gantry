/** Download buttons resolve through GitHub's stable "latest release" asset URLs; the enabled state and the version
 *  line come from the releases API (which sends CORS headers). Until a release with the stable asset names exists,
 *  everything stays in its "coming soon" state and every link still goes somewhere useful. */
import { detectOs, type Os } from './os';

const REPO = 'oljodev/gantry';
const ASSETS: Record<Os, string> = { mac: 'Gantry-macOS.dmg', win: 'Gantry-Windows-x64.exe', linux: 'Gantry-Linux-x86_64.AppImage' };

void (async () => {
  const buttons = document.querySelectorAll<HTMLAnchorElement>('a[data-dl]');
  const primary = document.querySelectorAll<HTMLAnchorElement>('a[data-dl-primary]');
  if (!buttons.length && !primary.length) return;
  const info = await fetchRelease();
  if (!info) return;
  document.documentElement.dataset.release = 'available';
  const url = (os: Os) => `https://github.com/${REPO}/releases/latest/download/${ASSETS[os]}`;
  for (const b of buttons) {
    const os = b.dataset.dl as Os;
    if (!info.assets.has(ASSETS[os])) continue;
    b.href = url(os);
    b.removeAttribute('aria-disabled');
    const s = b.querySelector('[data-state]'); if (s) s.textContent = 'Download';
  }
  const os = detectOs();
  for (const b of primary) {
    if (os && info.assets.has(ASSETS[os])) b.href = url(os);
  }
  document.querySelectorAll<HTMLElement>('[data-release-version]').forEach((el) => { el.textContent = `Version ${info.version}`; });
  document.querySelectorAll<HTMLElement>('[data-release-line]').forEach((el) => { el.textContent = `Version ${info.version}, released ${fmt(info.date)}`; el.hidden = false; });
})();

async function fetchRelease(): Promise<{ version: string; date: string; assets: Set<string> } | null> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 6000);
  try {
    const res = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`, { signal: controller.signal, headers: { Accept: 'application/vnd.github+json' } });
    if (!res.ok) return null;
    const api = (await res.json()) as { tag_name?: string; published_at?: string; assets?: { name: string }[] };
    if (!api.tag_name || !api.published_at) return null;
    const assets = new Set((api.assets ?? []).map((a) => a.name));
    if (![...Object.values(ASSETS)].some((n) => assets.has(n))) return null;
    return { version: api.tag_name.replace(/^v/, ''), date: api.published_at, assets };
  } catch { return null; } finally { clearTimeout(timer); }
}

function fmt(iso: string): string {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleDateString('en-GB', { day: 'numeric', month: 'long', year: 'numeric' });
}
