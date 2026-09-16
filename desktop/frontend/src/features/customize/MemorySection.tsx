import {
  ArrowCounterClockwiseIcon,
  BrainIcon,
  DownloadSimpleIcon,
  PlusIcon,
  TrashIcon,
  UploadSimpleIcon,
} from '@phosphor-icons/react';
import { useState } from 'react';

import { EmptyState } from '@/components/gantry/EmptyState';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Select } from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';
import { Textarea } from '@/components/ui/textarea';
import type { MemoryDto, MemoryKind, MemoryQuery, MemoryScopeKind } from '@/bindings';
import { commands, isTauri, unwrap } from '@/lib/ipc/client';
import { EMPTY_QUERY, useMemories, useMemoryMutations } from '@/lib/ipc/hooks/memory';
import { useProjects } from '@/lib/ipc/hooks/projects';
import { useSettings, useUpdateSettings } from '@/lib/ipc/hooks/settings';
import { describe } from '@/lib/errors';
import { cn } from '@/lib/utils';

const KINDS: [MemoryKind, string][] = [
  ['instruction', 'Instruction'],
  ['preference', 'Preference'],
  ['fact', 'Fact'],
  ['note', 'Note'],
];

/**
 * A scope as one value, because "global or a project, and which project" is one decision
 * (12 §B4). `global` or a project id; the two fields it writes are the store's shape, not a
 * thing to ask a person about twice.
 */
type Scope = string;

const GLOBAL: Scope = 'global';

function scopeOf(memory: { scope_kind: MemoryScopeKind; scope_id: string | null }): Scope {
  return memory.scope_kind === 'project' && memory.scope_id ? memory.scope_id : GLOBAL;
}

function scopeFields(scope: Scope): { scope_kind: MemoryScopeKind; scope_id: string | null } {
  return scope === GLOBAL
    ? { scope_kind: 'global', scope_id: null }
    : { scope_kind: 'project', scope_id: scope };
}

const KIND_HINT: Record<MemoryKind, string> = {
  instruction: 'How to behave. In every chat.',
  preference: 'A tool or style choice. In every chat.',
  fact: 'About you, your projects or your machine. Looked up when a message is about it.',
  note: 'A working note. Looked up when a message is about it.',
};

/**
 * Memory inside Customize (docs/plan/12 §B5, 15 A18).
 *
 * The page exists to keep one promise: **nothing reaches a prompt that is not a row here**. So
 * it shows every entry, says where each came from, and puts delete next to each one — with
 * Recently deleted behind it, because a memory deleted by mistake is a thing you notice later.
 */
export function MemorySection() {
  const [query, setQuery] = useState<MemoryQuery>(EMPTY_QUERY);
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string>();
  const entries = useMemories(query);
  const deleted = useMemories({ ...EMPTY_QUERY, archived: true });
  const { create, update, remove, restore, forgetForGood, importEntries } = useMemoryMutations();
  const settings = useSettings();
  const updateSettings = useUpdateSettings();
  const memory = settings.data?.memory;
  // A memory can belong to a project (12 §B4), which is a thing this page could neither show
  // nor set: every row looked global, and every row written here was.
  const projects = (useProjects().data ?? []).filter((p) => !p.archived);
  const scopes: { value: Scope; label: string }[] = [
    { value: GLOBAL, label: 'Everywhere' },
    ...projects.map((p) => ({ value: p.id, label: p.name })),
  ];
  const projectName = (id: string | null) =>
    projects.find((p) => p.id === id)?.name ?? 'a project that is gone';

  const setMemory = (patch: Partial<NonNullable<typeof memory>>) => {
    if (!memory) return;
    updateSettings.mutate({ memory: { ...memory, ...patch } });
  };

  const exportAll = async () => {
    const { save } = await import('@tauri-apps/plugin-dialog');
    try {
      const path = await save({
        title: 'Export memory',
        defaultPath: 'gantry-memory.json',
        filters: [{ name: 'JSON', extensions: ['json'] }],
      });
      if (!path) return;
      await unwrap(commands.exportMemories(path));
    } catch (err) {
      setError(describe(err));
    }
  };

  const importFile = async () => {
    const { open } = await import('@tauri-apps/plugin-dialog');
    try {
      const picked = await open({
        title: 'Import memory',
        filters: [{ name: 'JSON', extensions: ['json'] }],
      });
      if (typeof picked !== 'string') return;
      const json = await unwrap(commands.readMemoryExport(picked));
      const review = await unwrap(commands.reviewMemoryImport(json));
      const usable = review.entries.length - review.problems.length;
      if (usable <= 0) {
        setError('Nothing in that file could be imported.');
        return;
      }
      if (!window.confirm(`Add ${usable} memories from this file? Nothing is replaced.`)) return;
      importEntries.mutate(review.entries);
    } catch (err) {
      setError(describe(err));
    }
  };

  const rows = entries.data ?? [];
  const archived = deleted.data ?? [];

  return (
    <>
      <div className="mb-1 flex items-center gap-3">
        <h2 className="text-title font-medium text-fg">Memory</h2>
        <div className="ml-auto flex items-center gap-2">
          <Button variant="ghost" size="icon-sm" onClick={() => void importFile()} title="Import">
            <UploadSimpleIcon />
          </Button>
          <Button variant="ghost" size="icon-sm" onClick={() => void exportAll()} title="Export">
            <DownloadSimpleIcon />
          </Button>
          <Button onClick={() => setAdding(true)} disabled={!isTauri()}>
            <PlusIcon />
            Remember something
          </Button>
        </div>
      </div>
      <p className="mb-4 max-w-(--measure) text-body text-fg-2">
        What Gantry carries between chats. Every entry is here, nothing else reaches a prompt, and
        deleting one takes effect in every new chat immediately.
      </p>

      {memory && (
        <div className="mb-4 flex flex-col gap-3 rounded-3 border border-line-subtle bg-base px-4 py-3">
          <Row
            label="Pause memory"
            hint="Nothing is injected and nothing is proposed while this is on."
          >
            <Switch
              checked={memory.paused}
              onCheckedChange={(paused) => setMemory({ paused })}
              aria-label="Pause memory"
            />
          </Row>
          <Row
            label="Let Gantry propose memories"
            hint="It offers; you decide. Off means it never asks."
          >
            <Switch
              checked={memory.propose}
              onCheckedChange={(propose) => setMemory({ propose })}
              aria-label="Let Gantry propose memories"
            />
          </Row>
          <Row
            label="Remember and forget without asking"
            hint="A card still appears for every change, with Undo. Turn this off to be asked first."
          >
            <Switch
              checked={memory.auto_save_global}
              onCheckedChange={(auto_save_global) => setMemory({ auto_save_global })}
              aria-label="Remember and forget without asking"
            />
          </Row>
          {/* 12 §B3 says auto-save is per scope and the store has always had two fields; only
              one of them had a switch, so a project memory was saved without asking on a page
              that said asking was off. */}
          <Row
            label="…and in projects"
            hint="Project memories only ever reach chats in that project."
          >
            <Switch
              checked={memory.auto_save_project}
              onCheckedChange={(auto_save_project) => setMemory({ auto_save_project })}
              aria-label="Remember and forget without asking in projects"
            />
          </Row>
        </div>
      )}

      {error && (
        <p className="mb-3 rounded-2 border border-bad-subtle bg-bad-subtle px-3 py-2 text-meta text-bad">
          {error}
        </p>
      )}

      {adding && (
        <NewMemoryForm
          scopes={scopes}
          onCancel={() => setAdding(false)}
          saving={create.isPending}
          onSave={(text, kind, scope) =>
            create.mutate(
              {
                text,
                kind,
                ...scopeFields(scope),
                source: 'user',
                origin_chat_id: null,
                origin_message_id: null,
              },
              { onSuccess: () => setAdding(false) },
            )
          }
        />
      )}

      <div className="mb-3 flex flex-wrap items-center gap-2">
        <Input
          value={query.search}
          onChange={(e) => setQuery({ ...query, search: e.target.value })}
          placeholder="Search memory"
          aria-label="Search memory"
          className="max-w-64"
        />
        <Select
          value={query.kind ?? 'all'}
          onValueChange={(v) =>
            setQuery({ ...query, kind: v === 'all' ? null : (v as MemoryKind) })
          }
          aria-label="Filter by kind"
          items={[
            { value: 'all', label: 'Every kind' },
            ...KINDS.map(([value, label]) => ({ value, label })),
          ]}
        />
        <Select
          value={query.scope_kind ?? 'all'}
          onValueChange={(v) =>
            setQuery({ ...query, scope_kind: v === 'all' ? null : (v as MemoryScopeKind) })
          }
          aria-label="Filter by scope"
          items={[
            { value: 'all', label: 'Every scope' },
            { value: 'global', label: 'Everywhere' },
            { value: 'project', label: 'In a project' },
          ]}
        />
      </div>

      {!isTauri() ? (
        <p className="text-body text-fg-2">
          Not running inside the Gantry window, so there is nothing to read.
        </p>
      ) : rows.length === 0 ? (
        <EmptyState
          className="mt-10"
          icon={<BrainIcon />}
          title={query.search ? 'Nothing matches' : 'Nothing remembered yet'}
          hint={
            query.search
              ? 'Try another word.'
              : 'Say "remember that…" in a chat, or add one here. Gantry only keeps what you approve.'
          }
          action={
            <Button variant="secondary" onClick={() => setAdding(true)}>
              Remember something
            </Button>
          }
        />
      ) : (
        <div className="flex flex-col gap-2">
          {rows.map((m) => (
            <MemoryRow
              key={m.id}
              memory={m}
              scopes={scopes}
              scopeLabel={m.scope_kind === 'project' ? projectName(m.scope_id) : null}
              onEdit={(text, scope) =>
                update.mutate({ id: m.id, patch: { ...blank(), text, ...scopeFields(scope) } })
              }
              onToggle={(enabled) => update.mutate({ id: m.id, patch: { ...blank(), enabled } })}
              onAlways={(always_include) =>
                update.mutate({ id: m.id, patch: { ...blank(), always_include } })
              }
              onDelete={() => remove.mutate(m.id)}
            />
          ))}
        </div>
      )}

      {archived.length > 0 && (
        <details className="mt-6 rounded-3 border border-line-subtle bg-base px-4 py-3">
          <summary className="cursor-pointer text-ui font-medium text-fg">
            Recently deleted ({archived.length})
          </summary>
          <p className="mt-1 mb-2 text-meta text-fg-3">
            Kept for thirty days, then gone. Nothing here reaches a prompt.
          </p>
          <div className="flex flex-col gap-1.5">
            {archived.map((m) => (
              <div key={m.id} className="flex items-center gap-3 text-meta">
                <span className="min-w-0 flex-1 truncate text-fg-2">{m.text}</span>
                <Button variant="ghost" size="sm" onClick={() => restore.mutate(m.id)}>
                  <ArrowCounterClockwiseIcon />
                  Restore
                </Button>
                <Button variant="ghost" size="sm" onClick={() => forgetForGood.mutate(m.id)}>
                  Delete for good
                </Button>
              </div>
            ))}
          </div>
        </details>
      )}
    </>
  );
}

/** Every field of a patch left alone; the caller sets the one it means. */
function blank() {
  return {
    text: null,
    kind: null,
    scope_kind: null,
    scope_id: null,
    always_include: null,
    enabled: null,
    tags: null,
  };
}

function Row({
  label,
  hint,
  children,
}: {
  label: string;
  hint: string;
  children: React.ReactNode;
}) {
  return (
    <label className="flex items-start justify-between gap-4">
      <span className="min-w-0">
        <span className="block text-ui font-medium text-fg">{label}</span>
        <span className="block text-meta text-fg-2">{hint}</span>
      </span>
      {children}
    </label>
  );
}

function NewMemoryForm({
  scopes,
  onSave,
  onCancel,
  saving,
}: {
  scopes: { value: Scope; label: string }[];
  onSave: (text: string, kind: MemoryKind, scope: Scope) => void;
  onCancel: () => void;
  saving: boolean;
}) {
  const [text, setText] = useState('');
  const [kind, setKind] = useState<MemoryKind>('fact');
  const [scope, setScope] = useState<Scope>(GLOBAL);
  return (
    <div className="mb-4 flex flex-col gap-3 rounded-3 border border-accent-subtle bg-surface p-3">
      <Textarea
        autoFocus
        rows={2}
        value={text}
        maxLength={500}
        onChange={(e) => setText(e.target.value)}
        placeholder="One sentence that will still be true next week."
        aria-label="What to remember"
      />
      <div className="flex flex-wrap items-center gap-2">
        <Select
          value={kind}
          onValueChange={(v) => setKind(v as MemoryKind)}
          aria-label="Kind"
          items={KINDS.map(([value, label]) => ({ value, label }))}
        />
        {scopes.length > 1 && (
          <Select
            value={scope}
            onValueChange={(v) => setScope(v ?? GLOBAL)}
            aria-label="Where this applies"
            items={scopes}
          />
        )}
        <span className="min-w-0 flex-1 text-meta text-fg-3">{KIND_HINT[kind]}</span>
        <Button variant="ghost" onClick={onCancel}>
          Cancel
        </Button>
        <Button onClick={() => onSave(text, kind, scope)} disabled={text.trim() === '' || saving}>
          Remember
        </Button>
      </div>
    </div>
  );
}

function MemoryRow({
  memory,
  scopes,
  scopeLabel,
  onEdit,
  onToggle,
  onAlways,
  onDelete,
}: {
  memory: MemoryDto;
  scopes: { value: Scope; label: string }[];
  /** The project's name when this entry belongs to one, so the row can say so. */
  scopeLabel: string | null;
  onEdit: (text: string, scope: Scope) => void;
  onToggle: (enabled: boolean) => void;
  onAlways: (always: boolean) => void;
  onDelete: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(memory.text);
  const [scope, setScope] = useState<Scope>(scopeOf(memory));

  return (
    <div
      className={cn(
        'flex items-start gap-3 rounded-3 border border-line-subtle bg-surface p-3',
        !memory.enabled && 'opacity-60',
      )}
    >
      <div className="min-w-0 flex-1">
        {editing ? (
          <div className="flex flex-col gap-2">
            <Textarea
              autoFocus
              rows={2}
              value={draft}
              maxLength={500}
              onChange={(e) => setDraft(e.target.value)}
              aria-label="Memory text"
            />
            <div className="flex flex-wrap items-center gap-2">
              {scopes.length > 1 && (
                <Select
                  value={scope}
                  onValueChange={(v) => setScope(v ?? GLOBAL)}
                  aria-label="Where this applies"
                  items={scopes}
                />
              )}
              <Button
                size="sm"
                onClick={() => {
                  onEdit(draft.trim(), scope);
                  setEditing(false);
                }}
                disabled={draft.trim() === ''}
              >
                Save
              </Button>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => {
                  setDraft(memory.text);
                  setScope(scopeOf(memory));
                  setEditing(false);
                }}
              >
                Cancel
              </Button>
            </div>
          </div>
        ) : (
          <button
            type="button"
            onClick={() => setEditing(true)}
            className="block w-full text-left text-body text-fg hover:text-fg"
          >
            {memory.text}
          </button>
        )}
        <div className="mt-1 flex flex-wrap items-center gap-2 text-micro text-fg-3">
          <Badge variant="neutral">{memory.kind}</Badge>
          {scopeLabel !== null && <Badge variant="neutral">{scopeLabel}</Badge>}
          <span>
            {memory.source === 'user' ? 'You wrote this' : 'Gantry proposed it, you kept it'}
          </span>
          {memory.use_count > 0 && <span>· used {memory.use_count}×</span>}
          {memory.always_include && <Badge variant="accent">Always</Badge>}
        </div>
      </div>
      <div className="flex shrink-0 items-center gap-1">
        {!memory.kind.startsWith('instruction') && !memory.kind.startsWith('preference') && (
          <Button
            variant="ghost"
            size="sm"
            onClick={() => onAlways(!memory.always_include)}
            title="Keep this in every chat's prompt rather than looking it up"
          >
            {memory.always_include ? 'Always on' : 'Always?'}
          </Button>
        )}
        <Switch checked={memory.enabled} onCheckedChange={onToggle} aria-label="Use this memory" />
        <Button
          variant="ghost"
          size="icon-sm"
          onClick={onDelete}
          title="Delete"
          aria-label="Delete"
        >
          <TrashIcon />
        </Button>
      </div>
    </div>
  );
}
