import type { AttachmentInput } from '@/bindings';
import { isTauri } from '@/lib/ipc/client';

/** A file waiting in the composer's tray (docs/plan/01 §5, the `+` menu). */
export interface PendingAttachment {
  id: string;
  name: string;
  /** `image` when the mime says so, otherwise `file`; the backend does the real check. */
  kind: 'file' | 'image';
  input: AttachmentInput;
}

let counter = 0;
const nextId = () => `att-${Date.now()}-${counter++}`;

const IMAGE_EXT = /\.(png|jpe?g|gif|webp)$/i;

/** Paths from the file dialog or a drop on the window. */
export function fromPaths(paths: string[]): PendingAttachment[] {
  return paths.map((path) => {
    const name = path.split(/[\\/]/).pop() ?? path;
    return {
      id: nextId(),
      name,
      kind: IMAGE_EXT.test(name) ? 'image' : 'file',
      input: { kind: 'path', path },
    };
  });
}

/** Files the webview holds (pasted or dropped in a browser): bytes travel as base64. */
export async function fromFiles(files: Iterable<File>): Promise<PendingAttachment[]> {
  const out: PendingAttachment[] = [];
  for (const file of files) {
    const data_base64 = await toBase64(file);
    const name =
      file.name ||
      (file.type.startsWith('image/') ? `pasted.${file.type.split('/')[1]}` : 'pasted.txt');
    out.push({
      id: nextId(),
      name,
      kind: file.type.startsWith('image/') ? 'image' : 'file',
      input: { kind: 'bytes', name, mime: file.type, data_base64 },
    });
  }
  return out;
}

function toBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(reader.error);
    reader.onload = () => {
      const url = String(reader.result);
      resolve(url.slice(url.indexOf(',') + 1));
    };
    reader.readAsDataURL(file);
  });
}

/** Opens the native file picker; nothing in a browser. */
export async function pickFiles(): Promise<PendingAttachment[]> {
  if (!isTauri()) return [];
  const { open } = await import('@tauri-apps/plugin-dialog');
  const picked = await open({
    multiple: true,
    directory: false,
    title: 'Add files or images',
    filters: [
      {
        name: 'Text and images',
        extensions: [
          'txt',
          'md',
          'json',
          'toml',
          'yaml',
          'yml',
          'csv',
          'log',
          'rs',
          'ts',
          'tsx',
          'js',
          'py',
          'go',
          'html',
          'css',
          'xml',
          'sh',
          'png',
          'jpg',
          'jpeg',
          'gif',
          'webp',
        ],
      },
      { name: 'All files', extensions: ['*'] },
    ],
  });
  if (!picked) return [];
  return fromPaths(Array.isArray(picked) ? picked : [picked]);
}

/**
 * Files dropped on the window arrive as paths through Tauri's drag-drop event, not as DOM
 * `File`s. Returns the unlisten function.
 */
export async function onDroppedPaths(handler: (paths: string[]) => void): Promise<() => void> {
  if (!isTauri()) return () => undefined;
  const { getCurrentWebview } = await import('@tauri-apps/api/webview');
  return getCurrentWebview().onDragDropEvent((event) => {
    if (event.payload.type === 'drop' && event.payload.paths.length > 0) {
      handler(event.payload.paths);
    }
  });
}
