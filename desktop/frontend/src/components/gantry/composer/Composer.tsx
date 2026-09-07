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
  onDroppedPaths,
  type PendingAttachment,
  pickFiles,
} from '@/lib/attachments';
import { cn } from '@/lib/utils';

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
  const thinking = thinkingProp ?? thinkingLocal;
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
    if (more.length > 0) setAttachments((a) => [...a, ...more]);
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
            const files = Array.from(e.clipboardData.files);
            if (files.length === 0) return;
            e.preventDefault();
            void fromFiles(files).then(add);
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
              <DropdownMenuCheckboxItem checked={false} disabled>
                <GlobeIcon />
                Web search
              </DropdownMenuCheckboxItem>
              <DropdownMenuCheckboxItem checked={thinking} onCheckedChange={setThinking}>
                <BrainIcon />
                Thinking
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
            <Tooltip>
              <TooltipTrigger
                render={
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-pressed={thinking}
                    aria-label="Thinking"
                    onClick={() => setThinking(!thinking)}
                    className={cn(thinking && 'text-fg')}
                  />
                }
              >
                <BrainIcon weight={thinking ? 'fill' : 'regular'} />
              </TooltipTrigger>
              <TooltipContent>Thinking {thinking ? 'on' : 'off'}</TooltipContent>
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

/** Chips for the files waiting to go with the next message (15 §8, `AttachmentTray`). */
export function AttachmentTray({
  items,
  onRemove,
}: {
  items: PendingAttachment[];
  onRemove: (id: string) => void;
}) {
  return (
    <div className="mb-2 flex flex-wrap gap-1.5" aria-label="Attachments">
      {items.map((a) => (
        <span
          key={a.id}
          className="inline-flex h-6 max-w-64 items-center gap-1 rounded-2 border border-line bg-surface pr-1 pl-1.5 text-meta text-fg-2"
        >
          {a.kind === 'image' ? (
            <ImageIcon className="size-3.5 shrink-0" />
          ) : (
            <FileIcon className="size-3.5 shrink-0" />
          )}
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
