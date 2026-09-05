export type ThemeMode = 'light' | 'dark';
const mq = window.matchMedia('(prefers-color-scheme: dark)');
/** Follows the OS. `?theme=dark|light` in the URL stamps data-theme for previews and screenshots; there is no toggle on the page. */
export function currentTheme(): ThemeMode {
  const forced = document.documentElement.dataset.theme;
  if (forced === 'dark' || forced === 'light') return forced;
  return mq.matches ? 'dark' : 'light';
}
export function applyThemeOverride(): void {
  const wanted = new URLSearchParams(window.location.search).get('theme');
  if (wanted === 'dark' || wanted === 'light') document.documentElement.dataset.theme = wanted;
}
export function onThemeChange(cb: (mode: ThemeMode) => void): void {
  mq.addEventListener('change', () => cb(currentTheme()));
}
