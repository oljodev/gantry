import { FlaskIcon } from '@phosphor-icons/react';
import { useMemo, useState } from 'react';

import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { Textarea } from '@/components/ui/textarea';
import type { SkillDetail, SkillInput } from '@/bindings';
import { commands, isTauri, unwrap } from '@/lib/ipc/client';
import { describe } from '@/lib/errors';
import { cn } from '@/lib/utils';

const DESCRIPTION_MAX = 1024;
const BODY_MAX = 32 * 1024;

const TEMPLATE = `## When to use

The situation this applies to, in the words someone would use to describe it.

## Steps

1.
2.

## Example

## Pitfalls

-
`;

/** Lowercase, hyphenated: the name is also the folder name (12 §A2). */
function slugify(text: string) {
  return text
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 64);
}

/**
 * Writing a skill (docs/plan/12 §A5 flow 1): the fields on the left, the playbook on the right,
 * and a box that tells you whether a message would actually match it — which is the part of a
 * skill that is easiest to get wrong and hardest to notice.
 */
export function SkillEditor({
  skill,
  onCancel,
  onSave,
  saving,
  error,
}: {
  /** The skill being edited; absent when writing a new one. */
  skill?: SkillDetail;
  onCancel: () => void;
  onSave: (input: SkillInput) => void;
  saving: boolean;
  error?: string;
}) {
  // One field, typed freely, with the folder name shown under it as it is decided. A title and
  // a slug that follows it would need the slug to stop following once it is edited by hand,
  // and that is a rule nobody can see until it surprises them.
  const [typed, setTyped] = useState(skill?.name ?? '');
  const [description, setDescription] = useState(skill?.description ?? '');
  const [triggers, setTriggers] = useState((skill?.triggers ?? []).join(', '));
  const [always, setAlways] = useState(skill?.always_include ?? false);
  const [body, setBody] = useState(skill?.body ?? TEMPLATE);
  const isNew = skill === undefined;
  const name = slugify(typed);

  const input: SkillInput = useMemo(
    () => ({
      name,
      description: description.trim(),
      triggers: triggers
        .split(',')
        .map((t) => t.trim().toLowerCase())
        .filter(Boolean),
      always_include: always,
      author: skill?.author ?? null,
      license: skill?.license ?? null,
      body,
      references: [],
    }),
    [name, description, triggers, always, body, skill],
  );

  const problems = [
    name === '' && 'A name is required.',
    !isNew &&
      name !== skill.name &&
      `Saving under a new name writes a new skill; \`${skill.name}\` stays where it is.`,
    description.trim() === '' && 'A description is required; it is what matches a message.',
    body.trim() === '' && 'A skill is its instructions.',
  ].filter(Boolean) as string[];

  return (
    <div className="flex h-full flex-col">
      <div className="mb-4 flex items-center gap-3">
        <h2 className="text-title font-medium text-fg">{isNew ? 'New skill' : skill.name}</h2>
        <div className="ml-auto flex items-center gap-2">
          <Button variant="ghost" onClick={onCancel}>
            Cancel
          </Button>
          <Button onClick={() => onSave(input)} disabled={problems.length > 0 || saving}>
            {saving ? 'Saving…' : 'Save'}
          </Button>
        </div>
      </div>

      {error && (
        <p className="mb-3 rounded-2 border border-bad-subtle bg-bad-subtle px-3 py-2 text-meta text-bad">
          {error}
        </p>
      )}

      <div className="grid min-h-0 flex-1 grid-cols-1 gap-5 lg:grid-cols-[minmax(0,20rem)_minmax(0,1fr)]">
        <div className="flex flex-col gap-4 overflow-y-auto pr-1">
          <Field
            label="Name"
            hint={
              name === ''
                ? 'Lowercase letters, digits and hyphens. It is also the folder on disk.'
                : `Folder: ${name}`
            }
          >
            <Input
              value={typed}
              onChange={(e) => setTyped(e.target.value)}
              placeholder="rust-idioms"
              autoFocus={isNew}
            />
          </Field>
          <Field
            label="Description"
            hint={`What it does and when to use it — the matcher's main signal. ${description.length}/${DESCRIPTION_MAX}`}
          >
            <Textarea
              rows={4}
              value={description}
              maxLength={DESCRIPTION_MAX}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="Idiomatic Rust for this codebase. Use when writing or reviewing Rust, or when the user mentions the borrow checker or lifetimes."
            />
          </Field>
          <Field label="Triggers" hint="Comma-separated words and phrases that mean this skill.">
            <Input
              value={triggers}
              onChange={(e) => setTriggers(e.target.value)}
              placeholder="rust, borrow checker, lifetime"
            />
          </Field>
          <label className="flex items-start justify-between gap-4">
            <span>
              <span className="block text-ui font-medium text-fg">Always on</span>
              <span className="block text-meta text-fg-2">
                In every new chat's prompt instead of being matched. Costs its tokens every time.
              </span>
            </span>
            <Switch checked={always} onCheckedChange={setAlways} />
          </label>
          {!isNew && <MatchTester id={skill.id} />}
          {problems.length > 0 && (
            <ul className="text-meta text-fg-3">
              {problems.map((p) => (
                <li key={p}>{p}</li>
              ))}
            </ul>
          )}
        </div>

        <div className="flex min-h-0 flex-col">
          <div className="mb-1.5 flex items-baseline justify-between">
            <span className="text-ui font-medium text-fg">Instructions</span>
            <span className="font-mono text-micro text-fg-3">
              {body.length.toLocaleString()} / {BODY_MAX.toLocaleString()} characters
            </span>
          </div>
          <Textarea
            value={body}
            onChange={(e) => setBody(e.target.value)}
            maxLength={BODY_MAX}
            spellCheck={false}
            className="min-h-0 flex-1 resize-none font-mono text-meta leading-relaxed"
            aria-label="Skill instructions, in Markdown"
          />
        </div>
      </div>
    </div>
  );
}

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <label className="flex flex-col gap-1.5">
      <span className="text-ui font-medium text-fg">{label}</span>
      {children}
      {hint && <span className="text-meta text-fg-3">{hint}</span>}
    </label>
  );
}

/**
 * **Test match** (12 §A5): type a message, see the score and the words that earned it. The
 * scoring is the same function the turn uses, so what it says here is what will happen.
 */
function MatchTester({ id }: { id: string }) {
  const [message, setMessage] = useState('');
  const [result, setResult] = useState<{
    score: number;
    qualifies: boolean;
    hits: string[];
  } | null>(null);
  const [error, setError] = useState<string>();

  const run = async () => {
    if (!isTauri() || message.trim() === '') return;
    try {
      setResult(await unwrap(commands.testSkillMatch(id, message)));
      setError(undefined);
    } catch (err) {
      setError(describe(err));
    }
  };

  return (
    <div className="rounded-3 border border-line-subtle bg-base p-3">
      <div className="mb-2 flex items-center gap-1.5 text-ui font-medium text-fg">
        <FlaskIcon className="size-4 text-fg-2" />
        Test match
      </div>
      <div className="flex gap-2">
        <Input
          value={message}
          onChange={(e) => setMessage(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && void run()}
          placeholder="A message a user might send"
        />
        <Button variant="secondary" onClick={() => void run()} disabled={message.trim() === ''}>
          Test
        </Button>
      </div>
      {error && <p className="mt-2 text-meta text-bad">{error}</p>}
      {result && !error && (
        <p
          className={cn('mt-2 text-meta', result.qualifies ? 'text-good' : 'text-fg-2')}
          aria-live="polite"
        >
          {result.qualifies
            ? `Score ${result.score} — this message would load the skill.`
            : `Score ${result.score} — under 3, so it would not load.`}
          {result.hits.length > 0 && (
            <span className="text-fg-3"> Matched: {result.hits.join(', ')}.</span>
          )}
        </p>
      )}
    </div>
  );
}
