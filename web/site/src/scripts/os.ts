/** Marks the visitor's platform: the primary download button says which build it is, download cards highlight it. */
export type Os = 'mac' | 'win' | 'linux';
const NAMES: Record<Os, string> = { mac: 'macOS', win: 'Windows', linux: 'Linux' };

export function detectOs(): Os | null {
  const nav = navigator as Navigator & { userAgentData?: { platform?: string } };
  const p = (nav.userAgentData?.platform ?? navigator.platform ?? '').toLowerCase();
  const ua = navigator.userAgent.toLowerCase();
  if (p.includes('mac') || ua.includes('mac os')) return 'mac';
  if (p.includes('win') || ua.includes('windows')) return 'win';
  if (p.includes('linux') || ua.includes('x11')) return 'linux';
  return null;
}

const os = detectOs();
if (os) {
  document.documentElement.dataset.os = os;
  document.querySelectorAll<HTMLElement>('[data-os-name]').forEach((el) => { el.textContent = `for ${NAMES[os]}`; el.hidden = false; });
  document.querySelectorAll<HTMLElement>(`[data-os-card="${os}"]`).forEach((el) => el.setAttribute('data-primary', ''));
}
