export type Os = 'mac' | 'win' | 'linux';

/** Marks the visitor's platform as the primary download button. The other two stay one click away. */
export function markVisitorOs(): void {
  const os = detectOs();
  if (!os) return;
  document.querySelector<HTMLElement>(`[data-os="${os}"]`)?.setAttribute('data-primary', '');
}

function detectOs(): Os | null {
  const nav = navigator as Navigator & { userAgentData?: { platform?: string } };
  const p = (nav.userAgentData?.platform ?? navigator.platform ?? '').toLowerCase();
  const ua = navigator.userAgent.toLowerCase();
  if (p.includes('mac') || ua.includes('mac os')) return 'mac';
  if (p.includes('win') || ua.includes('windows')) return 'win';
  if (p.includes('linux') || ua.includes('linux') || ua.includes('x11')) return 'linux';
  return null;
}
