import {
  ArrowUpIcon,
  BrainIcon,
  FolderPlusIcon,
  FolderSimpleIcon,
  GlobeIcon,
  PaperclipIcon,
  PlugIcon,
  PlusIcon,
  SquareIcon,
  XIcon,
} from '@phosphor-icons/react';
import { useState } from 'react';

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
import { cn } from '@/lib/utils';

export interface ComposerProps {
  mode: Mode;
  guard: boolean;
  model: ModelRef;
  roots: string[];
  running?: boolean;
  placeholder?: string;
  onModeChange: (m: Mode) => void;
  onGuardChange: (g: boolean) => void;
  onModelChange: (m: ModelRef) => void;
  onSend?: (text: string) => void;
  onStop?: () => void;
}

/**
 * The floating composer (15 A13, §7): text on top, one toolbar row below with the + menu, mode
 * chip, model picker and root chips on the left; thinking and Send/Stop on the right.
 */
export function Composer({
  mode,
  guard,
  model,
  roots,
  running,
  placeholder,
  onModeChange,
  onGuardChange,
  onModelChange,
  onSend,
  onStop,
}: ComposerProps) {
  const [text, setText] = useState('');
  const [thinking, setThinking] = useState(true);
  const canSend = text.trim().length > 0 && !running;

  const send = () => {
    if (!canSend) return;
    onSend?.(text);
    setText('');
  };

  return (
    <div className="mx-auto w-full max-w-(--measure) px-6 pb-4">
      <div className="flex flex-col rounded-4 border border-line-subtle bg-raised p-3 shadow-none">
        <textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault();
              send();
            }
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
              <DropdownMenuItem>
                <PaperclipIcon />
                Add files or images
              </DropdownMenuItem>
              <DropdownMenuItem>
                <FolderPlusIcon />
                Add folder to workspace
              </DropdownMenuItem>
              <DropdownMenuItem>
                <PlugIcon />
                Connectors…
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuCheckboxItem checked={false}>
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
                    onClick={() => setThinking((t) => !t)}
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
