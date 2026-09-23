import { CheckCircleIcon, CircleDashedIcon, CircleIcon } from '@phosphor-icons/react';

import type { Todo } from '@/fixtures/types';
import { cn } from '@/lib/utils';

/**
 * The checklist a model keeps with `gantry__update_todos` (03 §9b): the newest version of it in
 * this turn, drawn where it was first written, so a long task can be followed without opening
 * the steps.
 *
 * The item in progress is the one place orange appears, and only while the turn runs (15 §3:
 * orange is for running). A turn that ended with an item still in progress leaves it drawn as
 * started, not as running — nothing is running any more, and a spinner would say otherwise.
 */
export function Checklist({ todos, running }: { todos: Todo[]; running: boolean }) {
  const done = todos.filter((t) => t.status === 'completed').length;
  return (
    <div className="w-full max-w-lg rounded-4 border border-line bg-raised py-3 pr-3 pl-3.5">
      <div className="mb-2 flex items-center justify-between text-meta">
        <span className="font-medium text-fg-2">Checklist</span>
        <span className="text-fg-3 tnum">
          {done} of {todos.length} done
        </span>
      </div>
      <ul className="flex flex-col gap-1.5">
        {todos.map((todo, i) => (
          <li key={i} className="flex items-start gap-2 text-body">
            <span className="mt-0.5 flex shrink-0 [&_svg]:size-4">
              <Glyph status={todo.status} running={running} />
            </span>
            <span
              className={cn(
                'min-w-0 break-words',
                todo.status === 'completed' && 'text-fg-3 line-through',
                todo.status === 'in_progress' && 'font-medium text-fg',
                todo.status === 'pending' && 'text-fg-2',
              )}
            >
              {todo.content}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}

function Glyph({ status, running }: { status: Todo['status']; running: boolean }) {
  switch (status) {
    case 'completed':
      return <CheckCircleIcon weight="fill" className="text-fg-3" aria-label="Done" />;
    case 'in_progress':
      return (
        <CircleDashedIcon
          className={cn(running ? 'animate-spin text-accent-text' : 'text-fg-2')}
          aria-label="In progress"
        />
      );
    default:
      return <CircleIcon className="text-fg-3" aria-label="Not started" />;
  }
}
