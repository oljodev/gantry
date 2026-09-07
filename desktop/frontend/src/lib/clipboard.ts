import { isTauri } from '@/lib/ipc/client';

/** Copies text through the clipboard plugin in the app and the web API in a browser. */
export async function copyText(text: string): Promise<void> {
  if (isTauri()) {
    const { writeText } = await import('@tauri-apps/plugin-clipboard-manager');
    await writeText(text);
    return;
  }
  await navigator.clipboard?.writeText(text);
}

/**
 * Opens an `https:` link in the system browser after the user confirms the full URL (13 §5:
 * links inside artifacts never navigate anything themselves).
 */
export async function openExternal(url: string): Promise<void> {
  if (!/^https:\/\//i.test(url)) return;
  if (!window.confirm(`Open this link in your browser?\n\n${url}`)) return;
  if (isTauri()) {
    const { openUrl } = await import('@tauri-apps/plugin-opener');
    await openUrl(url);
    return;
  }
  window.open(url, '_blank', 'noopener');
}
