import {
  ShieldCheckIcon,
  ShieldWarningIcon,
  ThumbsDownIcon,
  ThumbsUpIcon,
} from '@phosphor-icons/react';
import { Link } from '@tanstack/react-router';
import { useState } from 'react';

import type { GuardDecision } from '@/bindings';
import { Badge } from '@/components/ui/badge';
import { SettingsRow } from '@/components/gantry/settings/SettingsRow';
import { Button } from '@/components/ui/button';
import { ModelDialog } from '@/features/models/ModelDialog';
import { isTauri } from '@/lib/ipc/client';
import {
  useGuardDecisions,
  useMarkGuardDecision,
  useSettings,
  useUpdateSettings,
} from '@/lib/ipc/hooks/settings';
import { cn } from '@/lib/utils';

/**
 * Which model the guard asks (04 §6). Empty means the cheapest fast model of whichever provider
 * the chat is already using, from `judge_defaults.toml` — right for almost everybody, which is
 * why this is an override to reach for rather than a choice to make.
 */
export function JudgeModelRow() {
  const settings = useSettings();
  const update = useUpdateSettings();
  const [picking, setPicking] = useState(false);
  const chosen = settings.data?.guard?.judge_model ?? null;
  return (
    <SettingsRow
      label="Guard model"
      hint="The model that decides in Auto mode. By default, the cheapest fast model of the provider the chat is already using, so no second key is needed."
    >
      <div className="flex items-center gap-2">
        <Button variant="secondary" size="sm" onClick={() => setPicking(true)}>
          {chosen ? chosen.model : 'Provider default'}
        </Button>
        {chosen && (
          <Button
            variant="ghost"
            size="sm"
            onClick={() => update.mutate({ guard: { judge_model: null } })}
          >
            Reset
          </Button>
        )}
        <ModelDialog
          open={picking}
          onClose={() => setPicking(false)}
          value={chosen ?? { provider: 'openrouter', model: '' }}
          onChange={(model) => {
            update.mutate({ guard: { judge_model: model } });
            setPicking(false);
          }}
        />
      </div>
    </SettingsRow>
  );
}

/**
 * Settings → Guard, the record (04 §6, §11): what the guard has decided on the user's behalf,
 * newest first, with the reason it gave and a way to say it got one wrong.
 *
 * Allows are listed beside blocks on purpose. A page that showed only the blocks would answer
 * "what did it stop?" and leave "what has it been letting through?" — which is the question a
 * person actually has about something deciding for them — unanswered.
 */
export function GuardDecisions() {
  const decisions = useGuardDecisions();
  const mark = useMarkGuardDecision();

  if (!isTauri()) {
    return <p className="text-body text-fg-3">Decisions are shown in the app.</p>;
  }
  const rows = decisions.data ?? [];
  if (rows.length === 0) {
    return (
      <p className="text-body text-fg-3">
        Nothing yet. The guard decides when a chat is in Auto mode with Guard on; everything it
        decides is listed here.
      </p>
    );
  }
  const blocks = rows.filter((d) => d.verdict.decision === 'deny').length;
  const overrides = rows.filter((d) => d.verdict.overridden).length;

  return (
    <div className="flex flex-col gap-3">
      <p className="text-meta text-fg-3 tnum">
        Last {rows.length} decisions · {blocks} blocked ·{' '}
        {overrides === 0 ? 'none overruled' : `${overrides} overruled by you`}
      </p>
      <ul className="flex flex-col divide-y divide-line-subtle rounded-2 border border-line-subtle">
        {rows.map((d) => (
          <DecisionRow
            key={d.call_id}
            decision={d}
            onMark={(wrong) => mark.mutate({ callId: d.call_id, wrong })}
          />
        ))}
      </ul>
    </div>
  );
}

function DecisionRow({
  decision: d,
  onMark,
}: {
  decision: GuardDecision;
  onMark: (wrong: boolean | null) => void;
}) {
  const denied = d.verdict.decision === 'deny';
  const wrong = d.verdict.wrong;
  return (
    <li className="flex items-start gap-3 px-3 py-2.5">
      {denied ? (
        <ShieldWarningIcon className={cn('mt-0.5 size-4 shrink-0', 'text-bad')} />
      ) : (
        <ShieldCheckIcon className="mt-0.5 size-4 shrink-0 text-good" />
      )}
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <div className="flex flex-wrap items-center gap-1.5 text-ui text-fg">
          <span className="font-medium">
            {d.connector_name} · {d.tool}
          </span>
          <Badge variant={denied ? 'bad' : 'neutral'}>{denied ? 'Blocked' : 'Allowed'}</Badge>
          {d.verdict.overridden && <Badge variant="neutral">You allowed it</Badge>}
          {d.verdict.flags.map((f) => (
            <Badge key={f} variant="warn">
              {f.replace(/_/g, ' ')}
            </Badge>
          ))}
        </div>
        {d.summary && <p className="truncate font-mono text-micro text-fg-3">{d.summary}</p>}
        <p className="text-meta text-fg-2">{d.verdict.reason}</p>
        <p className="text-micro text-fg-3 tnum">
          <Link
            to="/chat/$chatId"
            params={{ chatId: d.chat_id }}
            className="underline-offset-2 hover:underline"
          >
            {d.chat_title || 'Untitled chat'}
          </Link>
          {d.verdict.model && ` · ${d.verdict.model}`}
          {d.verdict.source === 'loop' && ' · the loop detector, not a model'}
          {d.verdict.latency_ms > 0 && ` · ${(d.verdict.latency_ms / 1000).toFixed(1)} s`}
          {` · ${Math.round((d.verdict.confidence ?? 0) * 100)}% sure`}
        </p>
      </div>
      {/* 04 §6: stored with the decision, for the prompt tuning it is there for. Nothing reads
          it yet, and the hint under the section says so rather than implying an effect. */}
      <div className="flex shrink-0 items-center gap-0.5">
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="This decision was right"
          aria-pressed={wrong === false}
          onClick={() => onMark(wrong === false ? null : false)}
        >
          <ThumbsUpIcon className={cn('size-4', wrong === false ? 'text-good' : 'text-fg-3')} />
        </Button>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="This decision was wrong"
          aria-pressed={wrong === true}
          onClick={() => onMark(wrong === true ? null : true)}
        >
          <ThumbsDownIcon className={cn('size-4', wrong === true ? 'text-bad' : 'text-fg-3')} />
        </Button>
      </div>
    </li>
  );
}
