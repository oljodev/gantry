import { isTauri } from '@/lib/ipc/client';

/**
 * The native folder picker (16 §7, `docs/connectors/filesystem.md` §4).
 *
 * A root is only ever created from a folder the user picked here, never from model output —
 * which is why the workspace layer takes a path from this dialog and from nowhere else.
 * Returns `null` when the dialog is cancelled, and in a browser, where there is none.
 */
export async function pickFolder(title = 'Add folder to workspace'): Promise<string | null> {
  if (!isTauri()) return null;
  const { open } = await import('@tauri-apps/plugin-dialog');
  const picked = await open({ multiple: false, directory: true, title });
  return typeof picked === 'string' ? picked : null;
}

/** The last component, for a chip that has to fit next to the model picker. */
export function folderName(path: string): string {
  const parts = path.split(/[/\\]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}
