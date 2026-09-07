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
