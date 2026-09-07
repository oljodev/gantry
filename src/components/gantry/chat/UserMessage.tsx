import { FileIcon, ImageIcon } from '@phosphor-icons/react';

import type { Turn } from '@/fixtures/types';

/** The user's message: a tinted block, right-aligned, max 75 % of the measure (15 A14). */
export function UserMessage({ user }: { user: Turn['user'] }) {
  return (
    <div className="flex justify-end">
      <div className="selectable max-w-[75%] rounded-3 bg-raised px-4 py-3 text-chat text-fg ring-1 ring-line-subtle">
        {user.attachments && user.attachments.length > 0 && (
          <div className="mb-2 flex flex-wrap gap-1.5">
            {user.attachments.map((a) => (
              <span
                key={a.name}
                className="inline-flex h-6 items-center gap-1 rounded-2 border border-line bg-surface px-1.5 text-meta text-fg-2"
              >
                {a.kind === 'image' ? (
                  <ImageIcon className="size-3.5" />
                ) : (
                  <FileIcon className="size-3.5" />
                )}
                {a.name}
              </span>
            ))}
          </div>
        )}
        <div className="whitespace-pre-wrap">{user.text}</div>
      </div>
    </div>
  );
}
