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
export async function openExternal(url: string, confirmFirst = true): Promise<void> {
  if (!/^https?:\/\//i.test(url)) return;
  // A link inside an answer is the model's, so it is confirmed; a button in Gantry's own UI is
  // the user's own click and asking twice is noise.
  if (confirmFirst && !window.confirm(`Open this link in your browser?\n\n${url}`)) return;
  if (isTauri()) {
    const { openUrl } = await import('@tauri-apps/plugin-opener');
    await openUrl(url);
    return;
  }
  window.open(url, '_blank', 'noopener');
}

/**
 * An image on the clipboard, for pasting a screenshot into the composer. The webview's own
 * clipboard data is tried first; WebKitGTK often hands over nothing there, so the app falls
 * back to the clipboard plugin and encodes the raw pixels as a PNG itself.
 */
export async function readClipboardImage(): Promise<File | null> {
  if (!isTauri()) return null;
  try {
    const { readImage } = await import('@tauri-apps/plugin-clipboard-manager');
    const image = await readImage();
    const { width, height } = await image.size();
    const rgba = await image.rgba();
    if (width === 0 || height === 0 || rgba.length === 0) return null;
    const canvas = document.createElement('canvas');
    canvas.width = width;
    canvas.height = height;
    const context = canvas.getContext('2d');
    if (!context) return null;
    context.putImageData(new ImageData(new Uint8ClampedArray(rgba), width, height), 0, 0);
    const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, 'image/png'));
    if (!blob) return null;
    return new File([blob], `pasted-${Date.now()}.png`, { type: 'image/png' });
  } catch {
    // Nothing on the clipboard is an image, or the plugin refused: paste stays a no-op.
    return null;
  }
}
