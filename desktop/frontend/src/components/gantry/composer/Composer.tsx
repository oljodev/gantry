import {
  ArrowUpIcon,
  BrainIcon,
  FileIcon,
  FolderPlusIcon,
  FolderSimpleIcon,
  GlobeIcon,
  ImageIcon,
  PaperclipIcon,
  PlugIcon,
  PlusIcon,
  SquareIcon,
  XIcon,
} from '@phosphor-icons/react';
import { useEffect, useState } from 'react';

import { ImageLightbox } from '@/components/gantry/ImageLightbox';
import { ModeChip } from '@/components/gantry/composer/ModeChip';
import { ModelPicker } from '@/components/gantry/composer/ModelPicker';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Kbd } from '@/components/ui/kbd';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import type { Mode, ModelRef } from '@/fixtures/types';
import {
  fromFiles,
  fromPaths,
  loadPreviews,
  onDroppedPaths,
  type PendingAttachment,
  pickFiles,
} from '@/lib/attachments';
import { readClipboardImage } from '@/lib/clipboard';
import { cn } from '@/lib/utils';

/**
 * Files on a paste. `files` is empty on some platforms even when an image is there, so the
 * clipboard's items are checked as well before the app asks the system clipboard itself.
 */
function pastedFiles(data: DataTransfer): File[] {
  const files = Array.from(data.files);
  if (files.length > 0) return files;
  return Array.from(data.items)
    .filter((i) => i.kind === 'file')
    .map((i) => i.getAsFile())
    .filter((f): f is File => f !== null);
}

export interface ComposerProps {
  mode: Mode;
  guard: boolean;
  model: ModelRef;
  roots: string[];
  running?: boolean;
  placeholder?: string;
  /** Controlled thinking toggle; uncontrolled when absent (the gallery). */
  thinking?: boolean;
  onThinkingChange?: (on: boolean) => void;
  /** The provider's own web search (02 §3); shown only when the model has one. */
  webSearch?: boolean;
  onWebSearchChange?: (on: boolean) => void;
  /** What the current model can do; both default to on when the catalog does not know. */
  capabilities?: { thinking?: boolean; webSearch?: boolean };
  /** Text to place in the field; a new `nonce` re-applies the same text. */
  prefill?: { text: string; nonce: number };
  /** Attachments to show at first (the gallery). */
  initialAttachments?: PendingAttachment[];
  onModeChange: (m: Mode) => void;
  onGuardChange: (g: boolean) => void;
  onModelChange: (m: ModelRef) => void;
  onSend?: (text: string, attachments: PendingAttachment[]) => void;
  onStop?: () => void;
}

/**
 * The floating composer (15 A13, §7): the attachment tray, text, then one toolbar row with the
 * + menu, mode chip, model picker and root chips on the left; thinking and Send/Stop on the
 * right. Files arrive from the + menu, from paste, or dropped on the window.
 */
export function Composer({
  mode,
  guard,
  model,
  roots,
  running,
  placeholder,
  thinking: thinkingProp,
  onThinkingChange,
  webSearch = false,
  onWebSearchChange,
  capabilities,
  prefill,
  initialAttachments,
  onModeChange,
  onGuardChange,
  onModelChange,
  onSend,
  onStop,
}: ComposerProps) {
  const [text, setText] = useState('');
  const [attachments, setAttachments] = useState<PendingAttachment[]>(initialAttachments ?? []);
  const [thinkingLocal, setThinkingLocal] = useState(true);
  const canThink = capabilities?.thinking ?? true;
  const canSearch = capabilities?.webSearch ?? false;
  const thinking = canThink && (thinkingProp ?? thinkingLocal);
  const setThinking = (on: boolean) => {
    setThinkingLocal(on);
    onThinkingChange?.(on);
  };
  // A new prefill nonce replaces the draft; adjusting state during render avoids an extra pass.
  const [appliedNonce, setAppliedNonce] = useState<number | undefined>(undefined);
  if (prefill && prefill.nonce !== appliedNonce) {
    setAppliedNonce(prefill.nonce);
    setText(prefill.text);
  }
  const canSend = (text.trim().length > 0 || attachments.length > 0) && !running;

  const add = (more: PendingAttachment[]) => {
    if (more.length === 0) return;
    setAttachments((a) => [...a, ...more]);
    // An image chosen by path has no picture yet; the backend reads it for the thumbnail.
    void loadPreviews(more).then((previews) => {
      if (Object.keys(previews).length === 0) return;
      setAttachments((a) => a.map((x) => (previews[x.id] ? { ...x, preview: previews[x.id] } : x)));
    });
  };

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void onDroppedPaths((paths) => add(fromPaths(paths))).then((f) => {
      if (cancelled) f();
      else unlisten = f;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const send = () => {
    if (!canSend) return;
    onSend?.(text, attachments);
    setText('');
    setAttachments([]);
  };

  return (
    <div className="mx-auto w-full max-w-(--measure) px-6 pb-4">
      <div className="flex flex-col rounded-4 border border-line-subtle bg-raised p-3 shadow-none">
        {attachments.length > 0 && (
          <AttachmentTray
            items={attachments}
            onRemove={(id) => setAttachments((a) => a.filter((x) => x.id !== id))}
          />
        )}
        <textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault();
              send();
            }
          }}
          onPaste={(e) => {
            const files = pastedFiles(e.clipboardData);
            if (files.length > 0) {
              e.preventDefault();
              void fromFiles(files).then(add);
              return;
            }
            // WebKitGTK often hands over an empty file list for a screenshot on the clipboard,
            // so the app asks the clipboard itself before giving up.
            if (e.clipboardData.getData('text').length > 0) return;
            e.preventDefault();
            void readClipboardImage().then((file) => {
              if (file) void fromFiles([file]).then(add);
            });
          }}
          placeholder={placeholder ?? 'Message Gantry…'}
          aria-label="Message"
          rows={1}
          className="selectable field-sizing-content max-h-(--composer-max) min-h-6 w-full resize-none bg-transparent text-chat text-fg outline-none placeholder:text-fg-3"
        />
        <div className="mt-2 flex items-center gap-1">
          <DropdownMenu>
            <DropdownMenuTrigger
              render={<Button variant="ghost" size="icon-sm" aria-label="Add" />}
            >
              <PlusIcon />
            </DropdownMenuTrigger>
            <DropdownMenuContent className="w-64">
              <DropdownMenuItem onClick={() => void pickFiles().then(add)}>
                <PaperclipIcon />
                Add files or images
              </DropdownMenuItem>
              <DropdownMenuItem disabled>
                <FolderPlusIcon />
                Add folder to workspace
              </DropdownMenuItem>
              <DropdownMenuItem disabled>
                <PlugIcon />
                Connectors…
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuCheckboxItem
                checked={canSearch && webSearch}
                disabled={!canSearch}
                onCheckedChange={(on) => onWebSearchChange?.(on)}
              >
                <GlobeIcon />
                Web search
                {!canSearch && (
                  <span className="ml-auto text-meta text-fg-3">Not on this model</span>
                )}
              </DropdownMenuCheckboxItem>
              <DropdownMenuCheckboxItem
                checked={thinking}
                disabled={!canThink}
                onCheckedChange={setThinking}
              >
                <BrainIcon />
                Thinking
                {!canThink && (
                  <span className="ml-auto text-meta text-fg-3">Not on this model</span>
                )}
              </DropdownMenuCheckboxItem>
            </DropdownMenuContent>
          </DropdownMenu>
          <ModeChip
            mode={mode}
            guard={guard}
            onModeChange={onModeChange}
            onGuardChange={onGuardChange}
          />
          <ModelPicker value={model} onChange={onModelChange} />
          {roots.map((root) => (
            <RootChip key={root} root={root} />
          ))}
          <div className="ml-auto flex items-center gap-1">
            {canSearch && webSearch && (
              <Tooltip>
                <TooltipTrigger
                  render={
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      aria-pressed
                      aria-label="Web search"
                      onClick={() => onWebSearchChange?.(false)}
                      className="text-fg"
                    />
                  }
                >
                  <GlobeIcon weight="fill" />
                </TooltipTrigger>
                <TooltipContent>Web search on</TooltipContent>
              </Tooltip>
            )}
            <Tooltip>
              <TooltipTrigger
                render={
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-pressed={thinking}
                    aria-label="Thinking"
                    disabled={!canThink}
                    onClick={() => setThinking(!thinking)}
                    className={cn(thinking && 'text-fg')}
                  />
                }
              >
                <BrainIcon weight={thinking ? 'fill' : 'regular'} />
              </TooltipTrigger>
              <TooltipContent>
                {canThink ? `Thinking ${thinking ? 'on' : 'off'}` : 'This model does not think'}
              </TooltipContent>
            </Tooltip>
            {running ? (
              <Button
                variant="secondary"
                size="icon-md"
                aria-label="Stop"
                onClick={onStop}
                className="text-fg hover:bg-bad-subtle hover:text-bad"
              >
                <SquareIcon weight="fill" className="size-3" />
              </Button>
            ) : (
              <Tooltip>
                <TooltipTrigger
                  render={
                    <Button
                      variant="primary"
                      size="icon-md"
                      aria-label="Send"
                      disabled={!canSend}
                      onClick={send}
                    />
                  }
                >
                  <ArrowUpIcon weight="bold" />
                </TooltipTrigger>
                <TooltipContent>
                  Send <Kbd>↵</Kbd>
                </TooltipContent>
              </Tooltip>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

/**
 * What is waiting to go with the next message (15 §8, `AttachmentTray`): an image as a
 * thumbnail that opens full size on click, everything else as a chip. Both remove with the ×.
 */
export function AttachmentTray({
  items,
  onRemove,
}: {
  items: PendingAttachment[];
  onRemove: (id: string) => void;
}) {
  const [shown, setShown] = useState<PendingAttachment | null>(null);
  const images = items.filter((a) => a.kind === 'image');
  const files = items.filter((a) => a.kind !== 'image');
  return (
    <div className="mb-2 flex flex-col gap-1.5" aria-label="Attachments">
      {images.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {images.map((a) => (
            <div key={a.id} className="group/att relative">
              <button
                type="button"
                onClick={() => a.preview && setShown(a)}
                disabled={!a.preview}
                title={a.name}
                aria-label={`Open ${a.name}`}
                className="flex size-14 items-center justify-center overflow-hidden rounded-2 border border-line bg-surface enabled:cursor-zoom-in"
              >
                {a.preview ? (
                  <img src={a.preview} alt={a.name} className="size-full object-cover" />
                ) : (
                  <ImageIcon className="size-4 text-fg-3" />
                )}
              </button>
              <button
                type="button"
                aria-label={`Remove ${a.name}`}
                onClick={() => onRemove(a.id)}
                className="absolute -top-1 -right-1 flex size-4.5 items-center justify-center rounded-full border border-line bg-overlay text-fg-2 opacity-0 transition-opacity duration-(--dur-1) group-hover/att:opacity-100 hover:text-fg focus-visible:opacity-100"
              >
                <XIcon className="size-3" />
              </button>
            </div>
          ))}
        </div>
      )}
      {files.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {files.map((a) => (
            <span
              key={a.id}
              className="inline-flex h-6 max-w-64 items-center gap-1 rounded-2 border border-line bg-surface pr-1 pl-1.5 text-meta text-fg-2"
            >
              <FileIcon className="size-3.5 shrink-0" />
              <span className="truncate" title={a.name}>
                {a.name}
              </span>
              <button
                type="button"
                aria-label={`Remove ${a.name}`}
                onClick={() => onRemove(a.id)}
                className="ml-0.5 flex size-4 items-center justify-center rounded-1 text-fg-3 hover:bg-hover hover:text-fg"
              >
                <XIcon className="size-3" />
              </button>
            </span>
          ))}
        </div>
      )}
      <ImageLightbox
        src={shown?.preview ?? null}
        alt={shown?.name}
        open={shown !== null}
        onClose={() => setShown(null)}
      />
    </div>
  );
}

function RootChip({ root }: { root: string }) {
  return (
    <span className="inline-flex h-(--control-sm) items-center gap-1 rounded-2 border border-line-subtle px-1.5 text-meta text-fg-2">
      <FolderSimpleIcon className="size-3.5" />
      <span className="font-mono">{root}</span>
      <button
        type="button"
        aria-label={`Remove ${root}`}
        className="rounded-1 text-fg-3 hover:text-fg"
      >
        <XIcon className="size-3" />
      </button>
    </span>
  );
}
