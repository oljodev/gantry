import {
  DownloadSimpleIcon,
  GraduationCapIcon,
  LinkIcon,
  PencilSimpleIcon,
  PlusIcon,
  PushPinIcon,
  TrashIcon,
  UploadSimpleIcon,
} from '@phosphor-icons/react';
import { useState } from 'react';

import { EmptyState } from '@/components/gantry/EmptyState';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Switch } from '@/components/ui/switch';
import type { SkillDto, SkillReview } from '@/bindings';
import { ImportReview } from '@/features/skills/ImportReview';
import { SkillEditor } from '@/features/skills/SkillEditor';
import { commands, isTauri, unwrap } from '@/lib/ipc/client';
import { useSkill, useSkillMutations, useSkills } from '@/lib/ipc/hooks/skills';
import { describe } from '@/lib/errors';
import { cn } from '@/lib/utils';

type View = { kind: 'list' } | { kind: 'new' } | { kind: 'edit'; id: string };

/**
 * Skills inside Customize (docs/plan/12 §A6, 15 A18): the library, the editor, and the three
 * ways a skill arrives — written here, imported, or proposed by the model in a chat.
 *
 * A skill is text and only text. Nothing on this screen can run anything, which is why an
 * import is a review screen rather than a permission prompt (12 §A1).
 */
export function SkillsSection() {
  const [view, setView] = useState<View>({ kind: 'list' });
  const [query, setQuery] = useState('');
  const [review, setReview] = useState<SkillReview | null>(null);
  const [error, setError] = useState<string>();
  const skills = useSkills();
  const editing = useSkill(view.kind === 'edit' ? view.id : null);
  const { save, remove, setEnabled, install } = useSkillMutations();

  const all = skills.data ?? [];
  const q = query.trim().toLowerCase();
  const shown = all.filter(
    (s) =>
      !q ||
      s.name.toLowerCase().includes(q) ||
      s.description.toLowerCase().includes(q) ||
      s.triggers.some((t) => t.includes(q)),
  );

  const importFrom = async (source: { path?: string; text?: string; url?: string }) => {
    setError(undefined);
    try {
      setReview(
        source.url
          ? await unwrap(commands.reviewSkillUrl(source.url))
          : await unwrap(commands.reviewSkill(source.path ?? null, source.text ?? null)),
      );
    } catch (err) {
      setError(describe(err));
    }
  };

  /** Import from a file or a folder the user picks; a pasted URL is the other door. */
  const pickFile = async () => {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const picked = await open({
      title: 'Import a skill',
      filters: [{ name: 'Skill', extensions: ['md'] }],
      directory: false,
    });
    if (typeof picked === 'string') await importFrom({ path: picked });
  };

  const pickFolder = async () => {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const picked = await open({ title: 'Import a skill folder', directory: true });
    if (typeof picked === 'string') await importFrom({ path: picked });
  };

  const exportSkill = async (id: string) => {
    const { save: saveDialog } = await import('@tauri-apps/plugin-dialog');
    try {
      const path = await saveDialog({
        title: 'Export skill',
        defaultPath: await commands.skillExportFilename(id),
        filters: [{ name: 'Skill', extensions: ['md'] }],
      });
      if (!path) return;
      await unwrap(commands.exportSkill(id, path));
    } catch (err) {
      setError(describe(err));
    }
  };

  if (view.kind === 'new' || (view.kind === 'edit' && editing.data)) {
    return (
      <SkillEditor
        skill={view.kind === 'edit' ? editing.data : undefined}
        saving={save.isPending}
        error={save.error ? describe(save.error) : undefined}
        onCancel={() => setView({ kind: 'list' })}
        onSave={(input) => {
          save.mutate(input, { onSuccess: () => setView({ kind: 'list' }) });
        }}
      />
    );
  }

  return (
    <>
      <div className="mb-1 flex items-center gap-3">
        <h2 className="text-title font-medium text-fg">Skills</h2>
        <div className="ml-auto flex items-center gap-2">
          <Button variant="secondary" onClick={() => void pickFile()} disabled={!isTauri()}>
            <UploadSimpleIcon />
            Import
          </Button>
          <Button onClick={() => setView({ kind: 'new' })} disabled={!isTauri()}>
            <PlusIcon />
            New skill
          </Button>
        </div>
      </div>
      <p className="mb-4 max-w-(--measure) text-body text-fg-2">
        Playbooks Gantry reads when a message matches one. A skill is text: it can change how the
        model approaches a task, and it cannot run anything, reach the network or touch a file.
      </p>

      {error && (
        <p className="mb-3 rounded-2 border border-bad-subtle bg-bad-subtle px-3 py-2 text-meta text-bad">
          {error}
        </p>
      )}

      {all.length > 6 && (
        <input
          type="search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search skills"
          aria-label="Search skills"
          className="mb-3 h-(--control-md) w-full rounded-2 border border-line bg-surface px-3 text-ui text-fg placeholder:text-fg-3 focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-focus"
        />
      )}

      {!isTauri() ? (
        <p className="text-body text-fg-2">
          Not running inside the Gantry window, so there is no library to read.
        </p>
      ) : shown.length === 0 ? (
        <EmptyState
          className="mt-10"
          icon={<GraduationCapIcon />}
          title={q ? 'No skill matches' : 'No skills yet'}
          hint={
            q
              ? 'Try another word.'
              : 'Write one, import one, or ask in a chat and keep what the model proposes.'
          }
          action={
            <Button variant="secondary" onClick={() => setView({ kind: 'new' })}>
              New skill
            </Button>
          }
        />
      ) : (
        <div className="flex flex-col gap-2">
          {shown.map((skill) => (
            <SkillRow
              key={skill.id}
              skill={skill}
              onEdit={() => setView({ kind: 'edit', id: skill.id })}
              onExport={() => void exportSkill(skill.id)}
              onDelete={() => remove.mutate(skill.id)}
              onToggle={(enabled) => setEnabled.mutate({ id: skill.id, enabled })}
            />
          ))}
        </div>
      )}

      <div className="mt-6 flex items-center justify-between gap-4 rounded-3 border border-line-subtle bg-base px-4 py-3">
        <div>
          <div className="text-ui font-medium text-fg">Somewhere else?</div>
          <div className="text-meta text-fg-2">
            A skill folder, or a link to a raw Markdown file. Both land on the same review screen.
          </div>
        </div>
        <div className="flex shrink-0 gap-2">
          <Button variant="secondary" onClick={() => void pickFolder()} disabled={!isTauri()}>
            Folder
          </Button>
          <Button
            variant="secondary"
            onClick={() => {
              const url = window.prompt('Link to a raw SKILL.md');
              if (url) void importFrom({ url });
            }}
            disabled={!isTauri()}
          >
            <LinkIcon />
            Link
          </Button>
        </div>
      </div>

      {review && (
        <ImportReview
          review={review}
          installing={install.isPending}
          onCancel={() => setReview(null)}
          onInstall={(name) =>
            install.mutate(
              { name, text: review.text, references: review.input.references },
              { onSuccess: () => setReview(null) },
            )
          }
        />
      )}
    </>
  );
}

function SkillRow({
  skill,
  onEdit,
  onExport,
  onDelete,
  onToggle,
}: {
  skill: SkillDto;
  onEdit: () => void;
  onExport: () => void;
  onDelete: () => void;
  onToggle: (enabled: boolean) => void;
}) {
  const editable = skill.source !== 'bundled';
  return (
    <div
      className={cn(
        'group flex items-start gap-3 rounded-3 border border-line-subtle bg-surface p-3',
        !skill.enabled && 'opacity-60',
      )}
    >
      <GraduationCapIcon className="mt-0.5 size-4 shrink-0 text-fg-2" />
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-ui font-medium text-fg">{skill.name}</span>
          {skill.source === 'bundled' && <Badge variant="neutral">Built in</Badge>}
          {skill.always_include && <Badge variant="accent">Always on</Badge>}
          {skill.pinned_count > 0 && (
            <span className="inline-flex items-center gap-1 text-micro text-fg-3">
              <PushPinIcon className="size-3" />
              {skill.pinned_count}
            </span>
          )}
        </div>
        <p className="mt-0.5 line-clamp-2 text-meta text-fg-2">{skill.description}</p>
        <p className="mt-1 font-mono text-micro text-fg-3">
          {skill.use_count > 0
            ? `Used ${skill.use_count} ${skill.use_count === 1 ? 'time' : 'times'}`
            : 'Not used yet'}
          {skill.triggers.length > 0 && ` · ${skill.triggers.slice(0, 4).join(', ')}`}
        </p>
      </div>
      <div className="flex shrink-0 items-center gap-1">
        <Button
          variant="ghost"
          size="icon-sm"
          onClick={onEdit}
          aria-label={editable ? `Edit ${skill.name}` : `Read ${skill.name}`}
          title={editable ? 'Edit' : 'Read'}
        >
          <PencilSimpleIcon />
        </Button>
        <Button
          variant="ghost"
          size="icon-sm"
          onClick={onExport}
          aria-label={`Export ${skill.name}`}
          title="Export"
        >
          <DownloadSimpleIcon />
        </Button>
        {editable && (
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={onDelete}
            aria-label={`Delete ${skill.name}`}
            title="Delete"
          >
            <TrashIcon />
          </Button>
        )}
        <Switch
          checked={skill.enabled}
          onCheckedChange={onToggle}
          aria-label={`${skill.name} enabled`}
        />
      </div>
    </div>
  );
}
