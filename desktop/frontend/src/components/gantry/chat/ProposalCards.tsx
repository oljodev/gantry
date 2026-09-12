import { BrainIcon, GraduationCapIcon } from '@phosphor-icons/react';
import { useState } from 'react';

import { InteractionCard } from '@/components/gantry/chat/InteractionCard';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { Badge } from '@/components/ui/badge';
import type { MemoryProposal, SkillProposal } from '@/bindings';

/** What the user did with a skill card. The fields come back edited: the card is a form. */
export type SkillAnswer =
  { kind: 'save'; name: string; description: string; body: string } | { kind: 'discard' };

/**
 * A skill the model wrote (docs/plan/12 §A5 flow 4).
 *
 * Every field is editable, because a skill proposed mid-conversation is a first draft of
 * something the user will keep for months. A name that is already taken is never replaced
 * silently: the card says what it would replace and offers the free name beside it.
 */
export function SkillProposalCard({
  id,
  proposal,
  onAnswer,
  pending = false,
}: {
  id: string;
  proposal: SkillProposal;
  onAnswer?: (answer: SkillAnswer) => void;
  pending?: boolean;
}) {
  const [name, setName] = useState(proposal.input.name);
  const [description, setDescription] = useState(proposal.input.description);
  const [body, setBody] = useState(proposal.input.body);
  const [open, setOpen] = useState(false);
  const replaces = proposal.replaces;
  const replacing = replaces !== null && name === replaces.name;
  const bundled = replaces?.source === 'bundled';

  return (
    <InteractionCard
      mark={<GraduationCapIcon className="size-4 shrink-0 text-fg-2" />}
      title={
        replaces ? `Replace v${replaces.version} of \`${replaces.name}\`?` : `Keep this as a skill?`
      }
      actions={
        <>
          <Button
            variant="primary"
            disabled={pending || name === '' || (replacing && bundled)}
            onClick={() => onAnswer?.({ kind: 'save', name, description, body })}
          >
            {replacing ? 'Replace' : 'Save skill'}
          </Button>
          {replaces && (
            <Button
              variant="secondary"
              disabled={pending}
              onClick={() =>
                onAnswer?.({
                  kind: 'save',
                  name: proposal.suggested_name,
                  description,
                  body,
                })
              }
            >
              Save as `{proposal.suggested_name}`
            </Button>
          )}
          <Button
            variant="ghost"
            disabled={pending}
            onClick={() => onAnswer?.({ kind: 'discard' })}
          >
            Discard
          </Button>
          <button
            type="button"
            onClick={() => setOpen((v) => !v)}
            className="ml-auto text-meta text-fg-3 hover:text-fg-2"
          >
            {open ? 'Hide' : 'Edit'}
          </button>
        </>
      }
    >
      {proposal.reason && <p className="mb-2">{proposal.reason}</p>}
      {bundled && replacing && (
        <p className="mb-2 text-meta text-warn">
          `{replaces.name}` ships with Gantry and cannot be replaced. Save it under another name.
        </p>
      )}
      {open ? (
        <div className="flex flex-col gap-2">
          <Input
            value={name}
            onChange={(e) =>
              setName(
                e.target.value
                  .toLowerCase()
                  .replace(/[^a-z0-9-]+/g, '-')
                  .slice(0, 64),
              )
            }
            aria-label="Skill name"
          />
          <Textarea
            rows={2}
            value={description}
            maxLength={1024}
            onChange={(e) => setDescription(e.target.value)}
            aria-label="Description"
          />
          <Textarea
            rows={10}
            value={body}
            onChange={(e) => setBody(e.target.value)}
            spellCheck={false}
            className="font-mono text-meta"
            aria-label="Instructions"
          />
        </div>
      ) : (
        <>
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-mono text-meta text-fg">{name}</span>
            {proposal.input.triggers.slice(0, 4).map((t) => (
              <Badge key={t} variant="neutral">
                {t}
              </Badge>
            ))}
          </div>
          <p className="mt-1 text-meta text-fg-2">{description}</p>
          <pre className="mt-2 max-h-40 overflow-y-auto rounded-2 bg-inset px-3 py-2 font-mono text-micro text-fg-2">
            {body}
          </pre>
        </>
      )}
      <input type="hidden" value={id} />
    </InteractionCard>
  );
}

export type MemoryAnswer = { kind: 'save'; text: string } | { kind: 'discard' };

/**
 * A memory the model proposed, or one it would forget (12 §B3).
 *
 * The text is editable before it is kept, which is what makes confirmation cheap rather than
 * annoying: a proposal that is nearly right is one word away from being right. When auto-save
 * is on the entry is already stored and the card offers **Undo** instead — the rule is that no
 * memory exists without the user seeing it, not that they must click.
 */
export function MemoryProposalCard({
  proposal,
  onAnswer,
  pending = false,
}: {
  proposal: MemoryProposal;
  onAnswer?: (answer: MemoryAnswer) => void;
  pending?: boolean;
}) {
  const [text, setText] = useState(proposal.text);
  const forgetting = proposal.action === 'forget';

  return (
    <InteractionCard
      mark={<BrainIcon className="size-4 shrink-0 text-fg-2" />}
      title={
        forgetting
          ? 'Forget this?'
          : proposal.auto_saved
            ? 'Remembered'
            : proposal.target
              ? 'Update what you remember?'
              : 'Remember this?'
      }
      actions={
        forgetting ? (
          <>
            <Button
              variant="primary"
              disabled={pending}
              onClick={() => onAnswer?.({ kind: 'save', text })}
            >
              Forget it
            </Button>
            <Button
              variant="ghost"
              disabled={pending}
              onClick={() => onAnswer?.({ kind: 'discard' })}
            >
              Keep it
            </Button>
          </>
        ) : proposal.auto_saved ? (
          <>
            <Button
              variant="secondary"
              disabled={pending}
              onClick={() => onAnswer?.({ kind: 'discard' })}
            >
              Undo
            </Button>
            <Button
              variant="ghost"
              disabled={pending}
              onClick={() => onAnswer?.({ kind: 'save', text })}
            >
              Keep
            </Button>
          </>
        ) : (
          <>
            <Button
              variant="primary"
              disabled={pending || text.trim() === ''}
              onClick={() => onAnswer?.({ kind: 'save', text })}
            >
              Remember
            </Button>
            <Button
              variant="ghost"
              disabled={pending}
              onClick={() => onAnswer?.({ kind: 'discard' })}
            >
              Not this
            </Button>
          </>
        )
      }
    >
      {forgetting ? (
        <p className="text-body text-fg line-through decoration-fg-3">{proposal.text}</p>
      ) : (
        <Textarea
          rows={2}
          value={text}
          maxLength={500}
          onChange={(e) => setText(e.target.value)}
          aria-label="What to remember"
        />
      )}
      <p className="mt-2 text-meta text-fg-3">
        <Badge variant="neutral">{proposal.kind}</Badge> {proposal.reason}
      </p>
      {proposal.target && !forgetting && (
        <p className="mt-1 text-meta text-fg-3">Replaces: {proposal.target.text}</p>
      )}
    </InteractionCard>
  );
}
