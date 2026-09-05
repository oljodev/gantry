/** Download buttons resolve through GitHub's stable "latest release" asset URLs (docs/plan/14 §3).
 *  The version label and the enabled state come from the GitHub releases API, which sends CORS headers;
 *  fetching a file from github.com/…/releases/download does not, so there is no releases.json round-trip.
 *  If the API does not answer (no release yet, private repository, rate limit, offline) the buttons stay "coming soon". */
const REPO = 'oljodev/gantry';
const STABLE_ASSETS: Record<string, string> = {
  mac: 'Gantry-macOS.dmg',
  win: 'Gantry-Windows-x64.exe',
  linux: 'Gantry-Linux-x86_64.AppImage',
};

interface ReleaseInfo { version: string; date: string; assets: Record<string, string> }

export async function initReleases(): Promise<void> {
  const buttons = document.querySelectorAll<HTMLAnchorElement>('a[data-os]');
  if (!buttons.length) return;
  const info = await fetchRelease();
  if (!info) return;
  for (const button of buttons) {
    const os = button.dataset.os ?? '';
    const name = info.assets[os] ?? STABLE_ASSETS[os];
    if (!name) continue;
    button.href = `https://github.com/${REPO}/releases/latest/download/${name}`;
    button.removeAttribute('aria-disabled');
    const state = button.querySelector('[data-state]');
    if (state) state.textContent = 'Download';
  }
  const line = document.querySelector<HTMLElement>('[data-release]');
  if (line) {
    line.textContent = `Version ${info.version}, released ${formatDate(info.date)}`;
    line.hidden = false;
  }
}

async function fetchRelease(): Promise<ReleaseInfo | null> {
  const api = await getJson<{ tag_name?: string; published_at?: string; assets?: { name: string }[] }>(`https://api.github.com/repos/${REPO}/releases/latest`);
  if (!api?.tag_name || !api.published_at) return null;
  const names = new Set((api.assets ?? []).map((a) => a.name));
  const assets: Record<string, string> = {};
  for (const [os, name] of Object.entries(STABLE_ASSETS)) if (names.has(name)) assets[os] = name;
  if (!Object.keys(assets).length) return null;
  return { version: api.tag_name.replace(/^v/, ''), date: api.published_at, assets };
}

async function getJson<T>(url: string): Promise<T | null> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 6000);
  try {
    const res = await fetch(url, { signal: controller.signal, headers: { Accept: 'application/vnd.github+json' } });
    if (!res.ok) return null;
    return (await res.json()) as T;
  } catch {
    return null;
  } finally {
    clearTimeout(timer);
  }
}

function formatDate(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleDateString('en-GB', { day: 'numeric', month: 'long', year: 'numeric' });
}
