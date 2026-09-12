import { WarningIcon } from '@phosphor-icons/react';
import { useState } from 'react';

import { Markdown } from '@/components/gantry/markdown/Markdown';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import type { SkillReview } from '@/bindings';

/**
 * The review screen every import passes through (docs/plan/12 §A5 flow 3).
 *
 * Nothing has been written when this opens. That is the whole safety model for a skill somebody
 * else wrote: it cannot execute anything, so the risk is prose that steers the model badly, and
 * the answer to that risk is that a person read it first. So the body is rendered in full,
 * every repair and every dropped file is named, and **Install** is the only thing that writes.
 */
export function ImportReview({
  review,
  onCancel,
  onInstall,
  installing,
}: {
  review: SkillReview;
  onCancel: () => void;
  onInstall: (name: string) => void;
  installing: boolean;
}) {
  const [name, setName] = useState(review.name);
  const blocked = review.problems.length > 0;
  const collides = review.replaces !== null && name === review.replaces;

  return (
    <Dialog open onOpenChange={(next) => !next && onCancel()}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>Import a skill</DialogTitle>
          <DialogDescription>
            Nothing is written until you install it. Read what it tells the model first.
          </DialogDescription>
        </DialogHeader>
        <div className="flex max-h-[60vh] flex-col gap-4 overflow-y-auto">
          {blocked ? (
            <div className="rounded-3 border border-bad-subtle bg-bad-subtle p-3">
              <div className="mb-1 text-ui font-medium text-bad">
                This is not a skill Gantry reads
              </div>
              <ul className="text-meta text-fg-2">
                {review.problems.map((p) => (
                  <li key={p}>{p}</li>
                ))}
              </ul>
            </div>
          ) : (
            <>
              <label className="flex flex-col gap-1.5">
                <span className="text-ui font-medium text-fg">Install as</span>
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
                />
                {collides && (
                  <span className="text-meta text-warn">
                    `{review.replaces}` already exists and would be replaced. Change the name to
                    keep both.
                  </span>
                )}
              </label>

              <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1 text-meta">
                <dt className="text-fg-3">Description</dt>
                <dd className="text-fg-2">{review.input.description}</dd>
                {review.input.triggers.length > 0 && (
                  <>
                    <dt className="text-fg-3">Triggers</dt>
                    <dd className="text-fg-2">{review.input.triggers.join(', ')}</dd>
                  </>
                )}
                {review.input.references.length > 0 && (
                  <>
                    <dt className="text-fg-3">References</dt>
                    <dd className="text-fg-2">
                      {review.input.references.map((r) => r.file).join(', ')}
                    </dd>
                  </>
                )}
                <dt className="text-fg-3">Size</dt>
                <dd className="font-mono text-fg-2">
                  {(review.text.length / 1024).toFixed(1)} KB · about{' '}
                  {Math.round(review.text.length / 4).toLocaleString()} tokens
                </dd>
              </dl>
            </>
          )}

          {review.warnings.length > 0 && (
            <ul className="flex flex-col gap-1.5 rounded-3 border border-line-subtle bg-base p-3">
              {review.warnings.map((w) => (
                <li key={w} className="flex items-start gap-2 text-meta text-fg-2">
                  <WarningIcon className="mt-0.5 size-4 shrink-0 text-warn" />
                  <span>{w}</span>
                </li>
              ))}
            </ul>
          )}

          {!blocked && (
            <div>
              <div className="mb-1.5 text-ui font-medium text-fg">What it tells the model</div>
              <div className="max-h-72 overflow-y-auto rounded-3 border border-line-subtle bg-base px-4 py-3">
                <Markdown>{review.input.body}</Markdown>
              </div>
            </div>
          )}
        </div>

        <DialogFooter>
          <Button variant="ghost" onClick={onCancel}>
            Cancel
          </Button>
          <Button onClick={() => onInstall(name)} disabled={blocked || name === '' || installing}>
            {installing ? 'Installing…' : collides ? 'Replace' : 'Install'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
