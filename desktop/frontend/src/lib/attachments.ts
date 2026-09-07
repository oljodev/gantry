import type { AttachmentInput } from '@/bindings';
import { isTauri } from '@/lib/ipc/client';

/** A file waiting in the composer's tray (docs/plan/01 §5, the `+` menu). */
export interface PendingAttachment {
  id: string;
  name: string;
  /** `image` when the mime says so, otherwise `file`; the backend does the real check. */
  kind: 'file' | 'image';
  input: AttachmentInput;
  /** A `data:` URL for an image the tray can show right away; a path has none until read. */
  preview?: string;
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
    const isImage = file.type.startsWith('image/');
    out.push({
      id: nextId(),
      name,
      kind: isImage ? 'image' : 'file',
      input: { kind: 'bytes', name, mime: file.type, data_base64 },
      preview: isImage ? `data:${file.type};base64,${data_base64}` : undefined,
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
 * The picture behind a path-based image attachment, read by the backend because the webview
 * cannot open a file it only knows the path of. Returns nothing for anything that is not a
 * supported image, or when the file is too large to preview.
 */
export async function loadPreviews(
  items: PendingAttachment[],
): Promise<Record<string, string | undefined>> {
  if (!isTauri()) return {};
  const { commands } = await import('@/lib/ipc/client');
  const out: Record<string, string | undefined> = {};
  await Promise.all(
    items.map(async (item) => {
      if (item.kind !== 'image' || item.preview || item.input.kind !== 'path') return;
      const preview = await commands.imagePreview(item.input.path);
      if (preview) out[item.id] = preview;
    }),
  );
  return out;
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
