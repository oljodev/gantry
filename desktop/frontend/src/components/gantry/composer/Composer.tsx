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
import { Link } from '@tanstack/react-router';
import { useEffect, useRef, useState } from 'react';

import { ImageLightbox } from '@/components/gantry/ImageLightbox';
import { ModeChip } from '@/components/gantry/composer/ModeChip';
import { ModelPicker } from '@/components/gantry/composer/ModelPicker';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Kbd } from '@/components/ui/kbd';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import type { ReasoningEffort } from '@/bindings';
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
import { completeSlash, invokedSkills, slashQuery } from '@/lib/composer/slash';
import { folderName } from '@/lib/folders';
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

/**
 * The thinking levels, in the order they are offered. `off` is one of them rather than a
 * separate switch: "how hard should it think" has an answer that is *not at all*, and a
 * checkbox plus a level is two controls for one decision (docs/plan/15 A13).
 */
const EFFORTS: ReasoningEffort[] = ['off', 'low', 'medium', 'high', 'max'];
const EFFORT_LABEL: Record<ReasoningEffort, string> = {
  off: 'Off',
  low: 'Low',
  medium: 'Medium',
  high: 'High',
  max: 'Max',
};

export interface ComposerProps {
  mode: Mode;
  guard: boolean;
  model: ModelRef;
  /** The folders this chat may reach; the file tools work in these and nowhere else. */
  roots: string[];
  /** The project this chat is filed in, when it is in one (09 M11): its chip says so, because
   * instructions and knowledge the user cannot see on the screen are the thing to say out loud. */
  project?: { id: string; name: string };
  /** Opens the native folder picker. The menu item is disabled without it (the gallery). */
  onAddRoot?: () => void;
  onRemoveRoot?: (root: string) => void;
  running?: boolean;
  placeholder?: string;
  /** Controlled reasoning effort; uncontrolled when absent (the gallery). */
  effort?: ReasoningEffort;
  onEffortChange?: (effort: ReasoningEffort) => void;
  /** The provider's own web search (02 §3); shown only when the model has one. */
  webSearch?: boolean;
  onWebSearchChange?: (on: boolean) => void;
  /** What the current model can do; both default to on when the catalog does not know. */
  capabilities?: { thinking?: boolean; webSearch?: boolean };
  /** Text to place in the field; a new `nonce` re-applies the same text. */
  prefill?: { text: string; nonce: number };
  /** Attachments to show at first (the gallery). */
  initialAttachments?: PendingAttachment[];
  /** Installed connectors, and whether this chat has attached each one (03 §11). */
  connectors?: ConnectorChoice[];
  onConnectorChange?: (instanceId: string, attached: boolean) => void;
  /** Opens the Customize dialog, for when there is nothing to attach yet. */
  onBrowseConnectors?: () => void;
  /** Opens the project picker (09 M11). Absent on a screen with no chat to file yet. */
  onChooseProject?: () => void;
  onModeChange: (m: Mode) => void;
  onGuardChange: (g: boolean) => void;
  onModelChange: (m: ModelRef) => void;
  /** Skills that can be invoked with `/name` (12 §A4 rule 5, §A6). */
  skills?: SkillChoice[];
  onSend?: (text: string, attachments: PendingAttachment[], skills: string[]) => void;
  onStop?: () => void;
}

/** One skill as the `/` menu offers it. */
export interface SkillChoice {
  name: string;
  description: string;
}

/** One installed connector as the + menu offers it. */
export interface ConnectorChoice {
  id: string;
  name: string;
  attached: boolean;
  /** Connected and enabled; an unauthorized server is listed but cannot be attached. */
  ready: boolean;
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
  project,
  onAddRoot,
  onRemoveRoot,
  running,
  placeholder,
  effort: effortProp,
  onEffortChange,
  webSearch = false,
  onWebSearchChange,
  capabilities,
  prefill,
  initialAttachments,
  connectors,
  onConnectorChange,
  onBrowseConnectors,
  onChooseProject,
  onModeChange,
  onGuardChange,
  onModelChange,
  skills,
  onSend,
  onStop,
}: ComposerProps) {
  const [text, setText] = useState('');
  const [caret, setCaret] = useState(0);
  const [slashIndex, setSlashIndex] = useState(0);
  const field = useRef<HTMLTextAreaElement | null>(null);
  const [attachments, setAttachments] = useState<PendingAttachment[]>(initialAttachments ?? []);
  const [effortLocal, setEffortLocal] = useState<ReasoningEffort>('medium');
  const canThink = capabilities?.thinking ?? true;
  const canSearch = capabilities?.webSearch ?? false;
  const effort = canThink ? (effortProp ?? effortLocal) : 'off';
  const thinking = effort !== 'off';
  const setEffort = (next: ReasoningEffort) => {
    setEffortLocal(next);
    onEffortChange?.(next);
  };
  // The level the brain button turns thinking back on at: the last one in use, so that off and
  // on again is not a silent demotion to medium. Adjusted during render, the way the prefill
  // nonce below is, because it follows a prop rather than an event.
  const [lastOn, setLastOn] = useState<ReasoningEffort>('medium');
  if (effort !== 'off' && effort !== lastOn) setLastOn(effort);
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

  /**
   * A paste that carries something other than text becomes an attachment.
   *
   * The webview's own clipboard data is tried first. WebKitGTK hands over an empty file list for
   * a screenshot, so when there is no text either the app asks the system clipboard itself; that
   * path needs `clipboard-manager:allow-read-image` in the app's capabilities, without which it
   * fails silently and pasting a screenshot looks like a dead key.
   */
  const paste = (e: { clipboardData: DataTransfer | null; preventDefault: () => void }) => {
    const data = e.clipboardData;
    const files = data ? pastedFiles(data) : [];
    if (files.length > 0) {
      e.preventDefault();
      void fromFiles(files).then(add);
      return;
    }
    if (data && data.getData('text').length > 0) return;
    e.preventDefault();
    void readClipboardImage().then((file) => {
      if (file) void fromFiles([file]).then(add);
    });
  };

  // Pasting a screenshot with the focus anywhere but the text area still lands in the composer:
  // the picture was copied for this chat, and hunting for the caret first is a step for nothing.
  useEffect(() => {
    const onPaste = (e: ClipboardEvent) => {
      if (e.defaultPrevented) return;
      const target = e.target as HTMLElement | null;
      if (target?.isContentEditable || target instanceof HTMLInputElement) return;
      if (target instanceof HTMLTextAreaElement) return;
      paste(e);
    };
    window.addEventListener('paste', onPaste);
    return () => window.removeEventListener('paste', onPaste);
  });

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

  // The `/` menu (12 §A6). It opens only while the first token is being typed, so a slash in
  // a path is a slash; `slash.ts` holds that rule and is tested on its own.
  const names = (skills ?? []).map((s) => s.name);
  const query = slashQuery(text, caret);
  // `/remember` sits in the same menu and is not a skill: `invokedSkills` only knows the names
  // above, so typing it forces nothing and the composer treats the message as a command.
  const offered = [
    ...(skills ?? []),
    { name: 'remember', description: 'Keep the rest of this line as a memory' },
  ];
  const matches = query === null ? [] : offered.filter((s) => s.name.startsWith(query)).slice(0, 6);
  const menuOpen = matches.length > 0;

  const choose = (name: string) => {
    const [next, at] = completeSlash(text, caret, name);
    setText(next);
    setCaret(at);
    setSlashIndex(0);
    requestAnimationFrame(() => {
      field.current?.focus();
      field.current?.setSelectionRange(at, at);
    });
  };

  const send = () => {
    if (!canSend) return;
    onSend?.(text, attachments, invokedSkills(text, names));
    setText('');
    setAttachments([]);
    setCaret(0);
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
        {menuOpen && (
          <ul
            role="listbox"
            aria-label="Skills"
            className="mb-2 flex flex-col overflow-hidden rounded-3 border border-line-subtle bg-surface"
          >
            {matches.map((s, i) => (
              <li key={s.name}>
                <button
                  type="button"
                  role="option"
                  aria-selected={i === slashIndex % matches.length}
                  onMouseDown={(e) => {
                    e.preventDefault();
                    choose(s.name);
                  }}
                  className={cn(
                    'flex w-full items-baseline gap-2 px-3 py-1.5 text-left',
                    i === slashIndex % matches.length ? 'bg-selected' : 'hover:bg-hover',
                  )}
                >
                  <span className="shrink-0 font-mono text-meta text-fg">/{s.name}</span>
                  <span className="min-w-0 flex-1 truncate text-meta text-fg-3">
                    {s.description}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
        <textarea
          ref={field}
          value={text}
          onChange={(e) => {
            setText(e.target.value);
            setCaret(e.target.selectionStart ?? e.target.value.length);
            setSlashIndex(0);
          }}
          onSelect={(e) => setCaret(e.currentTarget.selectionStart ?? 0)}
          onKeyDown={(e) => {
            if (menuOpen && (e.key === 'ArrowDown' || e.key === 'ArrowUp')) {
              e.preventDefault();
              setSlashIndex((i) => i + (e.key === 'ArrowDown' ? 1 : matches.length - 1));
              return;
            }
            if (menuOpen && (e.key === 'Tab' || e.key === 'Enter')) {
              e.preventDefault();
              choose(matches[slashIndex % matches.length]!.name);
              return;
            }
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault();
              send();
            }
          }}
          onPaste={paste}
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
              <DropdownMenuItem disabled={!onAddRoot} onClick={() => onAddRoot?.()}>
                <FolderPlusIcon />
                Add folder to workspace
              </DropdownMenuItem>
              {/* Only where there is a chat to file: the welcome screen has none yet, and an
                  incognito one is gone the moment you leave it. */}
              {onChooseProject && (
                <DropdownMenuItem onClick={onChooseProject}>
                  <FolderSimpleIcon />
                  {project ? 'Move to project…' : 'Add to project…'}
                </DropdownMenuItem>
              )}
              <DropdownMenuSeparator />
              <DropdownMenuGroup>
                <DropdownMenuLabel>Connectors</DropdownMenuLabel>
                {connectors && connectors.length > 0 ? (
                  connectors.map((c) => (
                    <DropdownMenuCheckboxItem
                      key={c.id}
                      checked={c.attached}
                      disabled={!c.ready}
                      onCheckedChange={(on) => onConnectorChange?.(c.id, on)}
                    >
                      <PlugIcon />
                      {c.name}
                      {!c.ready && (
                        <span className="ml-auto text-meta text-fg-3">Not connected</span>
                      )}
                    </DropdownMenuCheckboxItem>
                  ))
                ) : (
                  <DropdownMenuItem onClick={onBrowseConnectors} disabled={!onBrowseConnectors}>
                    <PlugIcon />
                    Add a connector…
                  </DropdownMenuItem>
                )}
              </DropdownMenuGroup>
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
              {canThink ? (
                <DropdownMenuSub>
                  <DropdownMenuSubTrigger>
                    <BrainIcon />
                    Thinking
                    <span className="ml-auto pr-1 text-meta text-fg-3">{EFFORT_LABEL[effort]}</span>
                  </DropdownMenuSubTrigger>
                  <DropdownMenuSubContent>
                    <DropdownMenuRadioGroup
                      value={effort}
                      onValueChange={(v) => setEffort(v as ReasoningEffort)}
                    >
                      {EFFORTS.map((level) => (
                        <DropdownMenuRadioItem key={level} value={level}>
                          {EFFORT_LABEL[level]}
                        </DropdownMenuRadioItem>
                      ))}
                    </DropdownMenuRadioGroup>
                  </DropdownMenuSubContent>
                </DropdownMenuSub>
              ) : (
                <DropdownMenuItem disabled>
                  <BrainIcon />
                  Thinking
                  <span className="ml-auto text-meta text-fg-3">Not on this model</span>
                </DropdownMenuItem>
              )}
            </DropdownMenuContent>
          </DropdownMenu>
          <ModeChip
            mode={mode}
            guard={guard}
            onModeChange={onModeChange}
            onGuardChange={onGuardChange}
          />
          <ModelPicker value={model} onChange={onModelChange} />
          {project && <ProjectChip project={project} />}
          {roots.map((root) => (
            <RootChip key={root} root={root} onRemove={onRemoveRoot} />
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
                    onClick={() => setEffort(thinking ? 'off' : lastOn)}
                    className={cn(thinking && 'text-fg')}
                  />
                }
              >
                <BrainIcon weight={thinking ? 'fill' : 'regular'} />
              </TooltipTrigger>
              <TooltipContent>
                {canThink
                  ? `Thinking ${thinking ? EFFORT_LABEL[effort].toLowerCase() : 'off'}`
                  : 'This model does not think'}
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

/** The project chip: which project's instructions and knowledge this chat carries, and a way
 * to go and read them. */
function ProjectChip({ project }: { project: { id: string; name: string } }) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Link
            to="/projects/$projectId"
            params={{ projectId: project.id }}
            className="inline-flex h-(--control-sm) items-center gap-1 rounded-2 border border-line-subtle px-1.5 text-meta text-fg-2 transition-colors duration-(--dur-1) hover:bg-hover hover:text-fg"
          />
        }
      >
        <FolderSimpleIcon className="size-3.5" />
        <span className="max-w-32 truncate">{project.name}</span>
      </TooltipTrigger>
      <TooltipContent>
        In the project {project.name}: its instructions and knowledge apply here.
      </TooltipContent>
    </Tooltip>
  );
}

/** A folder chip: the name is enough to recognise it, the full path is one hover away. */
function RootChip({ root, onRemove }: { root: string; onRemove?: (root: string) => void }) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <span className="inline-flex h-(--control-sm) items-center gap-1 rounded-2 border border-line-subtle px-1.5 text-meta text-fg-2" />
        }
      >
        <FolderSimpleIcon className="size-3.5" />
        <span className="font-mono">{folderName(root)}</span>
        {onRemove && (
          <button
            type="button"
            aria-label={`Remove ${root}`}
            onClick={() => onRemove(root)}
            className="rounded-1 text-fg-3 hover:text-fg"
          >
            <XIcon className="size-3" />
          </button>
        )}
      </TooltipTrigger>
      <TooltipContent>{root}</TooltipContent>
    </Tooltip>
  );
}
