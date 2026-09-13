import {
  ChatCircleIcon,
  FileTextIcon,
  FolderSimpleIcon,
  PlusIcon,
  PushPinIcon,
  SparkleIcon,
  TrashIcon,
  XIcon,
} from '@phosphor-icons/react';
import { Link, useNavigate } from '@tanstack/react-router';
import { useState } from 'react';

import { ArtifactGlyph } from '@/components/gantry/chat/ArtifactCard';
import { EmptyState } from '@/components/gantry/EmptyState';
import { ProjectDetailsDialog } from '@/features/projects/ProjectDetailsDialog';
import { Button } from '@/components/ui/button';
import { Switch } from '@/components/ui/switch';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Textarea } from '@/components/ui/textarea';
import type { ProjectDetail, ProjectId, RiskTier } from '@/bindings';
import { typeInfo } from '@/features/artifacts/registry';
import { describe } from '@/lib/errors';
import { pickFiles } from '@/lib/attachments';
import { pickFolder } from '@/lib/folders';
import { useConnectors } from '@/lib/ipc/hooks/connectors';
import { useSkills } from '@/lib/ipc/hooks/skills';
import {
  useProject,
  useProjectArtifacts,
  useProjectChats,
  useProjectMutations,
} from '@/lib/ipc/hooks/projects';
import { MODE_LABEL, MODES } from '@/lib/modes';
import { relativeTime } from '@/lib/relativeTime';
import { cn } from '@/lib/utils';

/**
 * One project (docs/plan/09 M11): what it holds, and what every chat started in it begins with.
 *
 * The four tabs are the four things a project *is*. Instructions and knowledge are what the model
 * reads; Defaults are what the chat opens with; Chats and Artifacts are what came out of it.
 */
export function ProjectPage({ projectId }: { projectId: ProjectId }) {
  const project = useProject(projectId);
  const navigate = useNavigate();
  const { update, remove } = useProjectMutations();
  const [editing, setEditing] = useState(false);
  const data = project.data;

  if (project.isError) {
    return (
      <div className="mx-auto w-full max-w-(--measure) px-6 py-8">
        <EmptyState
          icon={<FolderSimpleIcon />}
          title="That project is not here"
          hint="It may have been deleted from another window."
          action={
            <Button variant="secondary" onClick={() => void navigate({ to: '/projects' })}>
              Back to projects
            </Button>
          }
        />
      </div>
    );
  }
  if (!data) return null;

  return (
    <div className="mx-auto w-full max-w-(--measure) px-6 py-8">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <button
            type="button"
            onClick={() => setEditing(true)}
            className="truncate rounded-2 text-left text-page font-semibold text-fg transition-colors duration-(--dur-1) hover:text-fg-2"
            title="Rename"
          >
            {data.name}
          </button>
          {data.description && <p className="mt-1 text-body text-fg-2">{data.description}</p>}
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <Button
            variant="ghost"
            size="sm"
            aria-label={data.pinned ? 'Unpin project' : 'Pin project'}
            onClick={() => update.mutate({ id: data.id, patch: { pinned: !data.pinned } })}
          >
            <PushPinIcon weight={data.pinned ? 'fill' : 'regular'} />
          </Button>
          <Button
            variant="ghost"
            size="sm"
            aria-label="Delete project"
            onClick={() => {
              remove.mutate(data.id, {
                onSuccess: () => void navigate({ to: '/projects' }),
              });
            }}
          >
            <TrashIcon />
          </Button>
          <Button
            size="sm"
            onClick={() => void navigate({ to: '/chat', search: { project: data.id } as never })}
          >
            <PlusIcon />
            New chat
          </Button>
        </div>
      </div>

      <Tabs defaultValue="chats" className="mt-6">
        <TabsList>
          <TabsTrigger value="chats">
            <ChatCircleIcon /> Chats
          </TabsTrigger>
          <TabsTrigger value="knowledge">
            <FileTextIcon /> Knowledge
          </TabsTrigger>
          <TabsTrigger value="instructions">Instructions</TabsTrigger>
          <TabsTrigger value="defaults">Defaults</TabsTrigger>
          <TabsTrigger value="artifacts">
            <SparkleIcon /> Artifacts
          </TabsTrigger>
        </TabsList>
        <TabsContent value="chats" className="pt-4">
          <ChatsTab projectId={data.id} />
        </TabsContent>
        <TabsContent value="knowledge" className="pt-4">
          <KnowledgeTab project={data} />
        </TabsContent>
        <TabsContent value="instructions" className="pt-4">
          {/* Keyed on the saved text: when it changes — this window's save, or another
              window's — the editor starts again from what is actually stored. */}
          <InstructionsTab key={data.instructions} project={data} />
        </TabsContent>
        <TabsContent value="defaults" className="pt-4">
          <DefaultsTab project={data} />
        </TabsContent>
        <TabsContent value="artifacts" className="pt-4">
          <ArtifactsTab projectId={data.id} />
        </TabsContent>
      </Tabs>
      {editing && <ProjectDetailsDialog project={data} onClose={() => setEditing(false)} />}
    </div>
  );
}

function ChatsTab({ projectId }: { projectId: ProjectId }) {
  const chats = useProjectChats(projectId);
  const list = chats.data ?? [];
  if (chats.isSuccess && list.length === 0) {
    return (
      <EmptyState
        icon={<ChatCircleIcon />}
        title="No chats here yet"
        hint="A chat started from this project carries its instructions, its knowledge and its defaults."
      />
    );
  }
  return (
    <ul className="flex flex-col divide-y divide-line-subtle">
      {list.map((c) => (
        <li key={c.id}>
          <Link
            to="/chat/$chatId"
            params={{ chatId: c.id }}
            className="flex items-center gap-3 rounded-2 px-2 py-2.5 transition-colors duration-(--dur-1) hover:bg-hover"
          >
            <span className="min-w-0 flex-1 truncate text-ui text-fg">{c.title}</span>
            <span className="shrink-0 text-meta text-fg-3 tnum">
              {relativeTime(c.last_message_at)}
            </span>
          </Link>
        </li>
      ))}
    </ul>
  );
}

function KnowledgeTab({ project }: { project: ProjectDetail }) {
  const { addFile, removeFile } = useProjectMutations();
  const [error, setError] = useState<string | null>(null);
  const total = project.files.reduce((n, f) => n + f.text_chars, 0);

  const add = async () => {
    setError(null);
    const picked = await pickFiles();
    for (const file of picked) {
      addFile.mutate(
        { id: project.id, file: file.input },
        { onError: (err) => setError(describe(err)) },
      );
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between gap-3">
        <p className="text-meta text-fg-2">
          Text from these files is in the prompt of every chat in this project.
          {project.files.length > 0 && ` About ${Math.round(total / 4 / 100) / 10}k tokens.`}
        </p>
        <Button variant="secondary" size="sm" onClick={() => void add()}>
          <PlusIcon />
          Add file
        </Button>
      </div>
      {error && <div className="text-meta text-bad">{error}</div>}
      {project.files.length === 0 ? (
        <EmptyState
          icon={<FileTextIcon />}
          title="No knowledge files"
          hint="Text, Markdown, code, CSV, JSON and PDFs. A picture cannot be knowledge: attach it to a message instead."
        />
      ) : (
        <ul className="flex flex-col divide-y divide-line-subtle">
          {project.files.map((f) => (
            <li key={f.id} className="group/row flex items-center gap-3 px-2 py-2.5">
              <FileTextIcon className="size-4 shrink-0 text-fg-3" />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-ui text-fg">{f.name}</span>
                <span className="block truncate text-meta text-fg-3 tnum">
                  {f.text_chars.toLocaleString()} characters of text
                </span>
              </span>
              <Button
                variant="ghost"
                size="sm"
                aria-label={`Remove ${f.name}`}
                className="opacity-0 group-hover/row:opacity-100"
                onClick={() => removeFile.mutate({ id: project.id, fileId: f.id })}
              >
                <XIcon />
              </Button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function InstructionsTab({ project }: { project: ProjectDetail }) {
  const { update } = useProjectMutations();
  const [text, setText] = useState(project.instructions);
  const [saved, setSaved] = useState(true);

  return (
    <div className="flex flex-col gap-3">
      <p className="text-meta text-fg-2">
        Standing instructions for every chat in this project. They sit under your global
        instructions and above nothing: a chat's own instructions still win.
      </p>
      <Textarea
        value={text}
        rows={10}
        maxLength={8000}
        placeholder="Answer in Norwegian. The API lives in services/api."
        onChange={(e) => {
          setText(e.target.value);
          setSaved(false);
        }}
      />
      <div className="flex items-center justify-between gap-3">
        <span className="text-meta text-fg-3 tnum">{text.length} / 8000</span>
        <Button
          size="sm"
          disabled={saved || update.isPending}
          onClick={() =>
            update.mutate(
              { id: project.id, patch: { instructions: text } },
              { onSuccess: () => setSaved(true) },
            )
          }
        >
          {saved ? 'Saved' : 'Save'}
        </Button>
      </div>
    </div>
  );
}

function DefaultsTab({ project }: { project: ProjectDetail }) {
  const { update, pinSkill } = useProjectMutations();
  const skills = useSkills();
  const connectors = useConnectors();
  const defaults = project.defaults;

  const setDefaults = (patch: Partial<typeof defaults>) =>
    update.mutate({ id: project.id, patch: { defaults: { ...defaults, ...patch } } });

  return (
    <div className="flex flex-col gap-6">
      <Row
        label="Folder"
        hint="Every chat here starts with this folder attached, and a code session works in it."
      >
        <div className="flex items-center gap-2">
          <span className="min-w-0 flex-1 truncate font-mono text-mono text-fg-2">
            {project.workspace_path ?? 'None'}
          </span>
          <Button
            variant="secondary"
            size="sm"
            onClick={() =>
              void pickFolder('Folder for this project').then((path) => {
                if (path) update.mutate({ id: project.id, patch: { workspace_path: path } });
              })
            }
          >
            Choose
          </Button>
          {project.workspace_path && (
            <Button
              variant="ghost"
              size="sm"
              aria-label="Remove folder"
              onClick={() => update.mutate({ id: project.id, patch: { workspace_path: null } })}
            >
              <XIcon />
            </Button>
          )}
        </div>
      </Row>

      <Row label="Permission mode" hint="Unset means whatever Settings says when the chat starts.">
        <div className="flex flex-wrap gap-1">
          <Chip
            active={defaults.mode === null}
            onClick={() => setDefaults({ mode: null })}
            label="From settings"
          />
          {MODES.map((m) => (
            <Chip
              key={m}
              active={defaults.mode === m}
              onClick={() => setDefaults({ mode: m })}
              label={MODE_LABEL[m]}
            />
          ))}
        </div>
      </Row>

      <Row label="Guard" hint="The second model that checks risky calls before they run.">
        <div className="flex items-center gap-3">
          <Switch
            checked={defaults.guard ?? false}
            onCheckedChange={(on) => setDefaults({ guard: on })}
          />
          <span className="text-meta text-fg-2">
            {defaults.guard === null ? 'From settings' : defaults.guard ? 'On' : 'Off'}
          </span>
          {defaults.guard !== null && (
            <Button variant="ghost" size="sm" onClick={() => setDefaults({ guard: null })}>
              Use the setting
            </Button>
          )}
        </div>
      </Row>

      <Row
        label="Connectors"
        hint="Attached to every new chat here, in place of the ones Settings would attach."
      >
        <div className="flex flex-col gap-1.5">
          {(connectors.data ?? []).length === 0 && (
            <span className="text-meta text-fg-3">No connectors installed yet.</span>
          )}
          {(connectors.data ?? []).map((c) => (
            <label key={c.id} className="flex items-center gap-2.5 text-ui text-fg">
              <Switch
                checked={(defaults.connectors ?? []).includes(c.namespace)}
                onCheckedChange={(on) => {
                  const chosen = new Set(defaults.connectors ?? []);
                  if (on) chosen.add(c.namespace);
                  else chosen.delete(c.namespace);
                  // An empty list is a decision — "attach nothing" — and null is the absence of
                  // one, so the two cannot be collapsed: the first is only reachable by turning
                  // one on and off again, which is exactly what a user who means it would do.
                  setDefaults({ connectors: [...chosen] });
                }}
              />
              <span className="min-w-0 flex-1 truncate">{c.name}</span>
              <span className="shrink-0 text-meta text-fg-3">{c.namespace}</span>
            </label>
          ))}
          {defaults.connectors !== null && (
            <Button
              variant="ghost"
              size="sm"
              className="self-start"
              onClick={() => setDefaults({ connectors: null })}
            >
              Use the setting instead
            </Button>
          )}
        </div>
      </Row>

      <Row
        label="Standing permissions"
        hint="What a chat here may do without asking. A permission prompt still appears for anything above the line, and the guardrails are never answered by one (04 §5)."
      >
        <div className="flex flex-col gap-1.5">
          {(connectors.data ?? []).length === 0 && (
            <span className="text-meta text-fg-3">No connectors installed yet.</span>
          )}
          {(connectors.data ?? []).map((c) => {
            const grant = (defaults.grants ?? []).find((g) => g.instance_name === c.namespace);
            const set = (ceiling: RiskTier | null) => {
              const rest = (defaults.grants ?? []).filter((g) => g.instance_name !== c.namespace);
              setDefaults({
                grants:
                  ceiling === null
                    ? rest
                    : [
                        ...rest,
                        { instance_name: c.namespace, tool_name: null, tier_ceiling: ceiling },
                      ],
              });
            };
            return (
              <div key={c.id} className="flex items-center gap-2.5 text-ui text-fg">
                <span className="min-w-0 flex-1 truncate">{c.name}</span>
                <div className="flex shrink-0 gap-1">
                  <Chip active={!grant} onClick={() => set(null)} label="Ask" />
                  <Chip
                    active={grant?.tier_ceiling === 'read'}
                    onClick={() => set('read')}
                    label="Reads"
                  />
                  <Chip
                    active={grant?.tier_ceiling === 'write'}
                    onClick={() => set('write')}
                    label="Reads and writes"
                  />
                </div>
              </div>
            );
          })}
        </div>
      </Row>

      <Row label="Pinned skills" hint="A pinned skill is in the prompt of every chat here, whole.">
        <div className="flex flex-col gap-1.5">
          {(skills.data ?? []).length === 0 && (
            <span className="text-meta text-fg-3">No skills installed yet.</span>
          )}
          {(skills.data ?? []).map((s) => (
            <label key={s.id} className="flex items-center gap-2.5 text-ui text-fg">
              <Switch
                checked={project.skills.includes(s.id)}
                onCheckedChange={(on) =>
                  pinSkill.mutate({ id: project.id, skillId: s.id, pinned: on })
                }
              />
              <span className="min-w-0 flex-1 truncate">{s.name}</span>
              <span className="truncate text-meta text-fg-3">{s.description}</span>
            </label>
          ))}
        </div>
      </Row>
    </div>
  );
}

function ArtifactsTab({ projectId }: { projectId: ProjectId }) {
  const artifacts = useProjectArtifacts(projectId);
  const list = artifacts.data ?? [];
  if (artifacts.isSuccess && list.length === 0) {
    return (
      <EmptyState
        icon={<SparkleIcon />}
        title="Nothing built here yet"
        hint="Documents, pages and diagrams made in this project's chats show up here, and any chat here can read them."
      />
    );
  }
  return (
    <ul className="flex flex-col divide-y divide-line-subtle">
      {list.map((a) => (
        <li key={a.id}>
          <Link
            to="/chat/$chatId"
            params={{ chatId: a.chat_id }}
            search={{ artifact: a.id }}
            className="flex items-center gap-3 rounded-2 px-2 py-2.5 transition-colors duration-(--dur-1) hover:bg-hover"
          >
            <span className="flex size-9 shrink-0 items-center justify-center rounded-2 bg-raised text-fg-2 [&_svg]:size-4.5">
              <ArtifactGlyph type={a.type} />
            </span>
            <span className="min-w-0 flex-1">
              <span className="block truncate text-ui font-medium text-fg">{a.title}</span>
              <span className="block truncate text-meta text-fg-3">
                {typeInfo(a.type)?.label ?? a.type} · v{a.current_version}
              </span>
            </span>
            <span className="shrink-0 text-meta text-fg-3 tnum">{relativeTime(a.updated_at)}</span>
          </Link>
        </li>
      ))}
    </ul>
  );
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
    <div className="flex flex-col gap-2">
      <div>
        <div className="text-ui font-medium text-fg">{label}</div>
        <div className="text-meta text-fg-2">{hint}</div>
      </div>
      {children}
    </div>
  );
}

function Chip({ active, label, onClick }: { active: boolean; label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        'h-(--control) rounded-2 border px-2.5 text-ui transition-colors duration-(--dur-1)',
        active
          ? 'border-transparent bg-selected text-fg'
          : 'border-line text-fg-2 hover:bg-hover hover:text-fg',
      )}
    >
      {label}
    </button>
  );
}
