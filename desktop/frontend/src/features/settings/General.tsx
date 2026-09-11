import { useState } from 'react';

import type { ChatSettings, Mode } from '@/bindings';
import { SettingsGroup, SettingsRow } from '@/components/gantry/settings/SettingsRow';
import { Button } from '@/components/ui/button';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';
import { Textarea } from '@/components/ui/textarea';
import { toast } from '@/components/ui/toast';
import { isTauri } from '@/lib/ipc/client';
import { useSettings, useUpdateSettings } from '@/lib/ipc/hooks/settings';
import { MODE_HINT, MODE_LABEL, MODES } from '@/lib/modes';
import { chatDefaults } from '@/lib/settingsDefaults';

/** Global custom instructions are capped at 4,000 characters (docs/plan/10 §2). */
const INSTRUCTIONS_MAX = 4000;

/**
 * Settings → General (11 §2): the defaults new chats start with, and the global custom
 * instructions that every new chat's prompt carries (existing chats get them as a note).
 */
export function General() {
  const settings = useSettings();
  const update = useUpdateSettings();
  if (!isTauri()) {
    return (
      <p className="text-body text-fg-2">
        Not running inside the Gantry window, so there are no settings to edit. They show here in
        the app.
      </p>
    );
  }
  if (!settings.data) return <p className="text-body text-fg-3">Loading…</p>;
  const chat = chatDefaults(settings.data);
  const patch = (next: Partial<ChatSettings>) => update.mutate({ chat: { ...chat, ...next } });

  return (
    <div className="flex flex-col gap-8">
      <SettingsGroup title="New chats">
        <SettingsRow label="Permission mode" hint={MODE_HINT[chat.default_mode]}>
          <Select
            value={chat.default_mode}
            onValueChange={(v) => v && patch({ default_mode: v as Mode })}
          >
            <SelectTrigger aria-label="Default permission mode">
              <SelectValue>{(v: Mode) => MODE_LABEL[v]}</SelectValue>
            </SelectTrigger>
            <SelectContent>
              {MODES.map((m) => (
                <SelectItem key={m} value={m}>
                  {MODE_LABEL[m]}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </SettingsRow>
        <SettingsRow
          label="Guard Auto mode"
          hint="In Auto mode, a small fast model decides each call that changes something, instead of asking you. It cannot reach past the guardrails."
        >
          <Switch
            aria-label="Guard Auto mode"
            checked={chat.default_guard}
            onCheckedChange={(v) => patch({ default_guard: v })}
          />
        </SettingsRow>
        <SettingsRow
          label="Open artifacts automatically"
          hint="Open the side panel the first time a reply creates an artifact."
        >
          <Switch
            aria-label="Open artifacts automatically"
            checked={chat.open_artifact_panel}
            onCheckedChange={(v) => patch({ open_artifact_panel: v })}
          />
        </SettingsRow>
        <SettingsRow
          label="Suggest connectors"
          hint="Let the assistant look through the catalog and offer to install a connector when one would help. It never installs anything by itself."
        >
          <Switch
            aria-label="Suggest connectors"
            checked={chat.suggest_connectors}
            onCheckedChange={(v) => patch({ suggest_connectors: v })}
          />
        </SettingsRow>
      </SettingsGroup>
      <SettingsGroup title="Custom instructions">
        <SettingsRow
          stacked
          label="What every chat should know about you"
          hint="Language, tone, conventions, context. New chats start with it; open chats are told about the change."
        >
          <InstructionsEditor
            value={chat.custom_instructions}
            saving={update.isPending}
            onSave={(text) =>
              update.mutate(
                { chat: { ...chat, custom_instructions: text } },
                { onSuccess: () => toast.add({ title: 'Instructions saved', type: 'success' }) },
              )
            }
          />
        </SettingsRow>
      </SettingsGroup>
    </div>
  );
}

function InstructionsEditor({
  value,
  saving,
  onSave,
}: {
  value: string;
  saving: boolean;
  onSave: (text: string) => void;
}) {
  const [draft, setDraft] = useState(value);
  // A save elsewhere (or the first load) replaces the draft; adjusting state during render
  // avoids an extra pass (the React docs' pattern for derived state).
  const [seen, setSeen] = useState(value);
  if (seen !== value) {
    setSeen(value);
    setDraft(value);
  }
  const dirty = draft.trim() !== value.trim();
  const over = draft.length > INSTRUCTIONS_MAX;
  return (
    <div className="flex flex-col gap-2">
      <Textarea
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        placeholder="Answer in Norwegian. Prefer idiomatic Rust. Never use emoji."
        aria-label="Custom instructions"
        aria-invalid={over || undefined}
        className="min-h-32 font-sans"
      />
      <div className="flex items-center justify-between">
        <span className={over ? 'text-meta text-bad tnum' : 'text-meta text-fg-3 tnum'}>
          {draft.length.toLocaleString()} / {INSTRUCTIONS_MAX.toLocaleString()} characters · about{' '}
          {Math.ceil(draft.length / 4).toLocaleString()} tokens
        </span>
        <div className="flex gap-1">
          <Button
            variant="ghost"
            size="sm"
            disabled={!dirty || saving}
            onClick={() => setDraft(value)}
          >
            Revert
          </Button>
          <Button
            variant="primary"
            size="sm"
            disabled={!dirty || over || saving}
            onClick={() => onSave(draft.trim())}
          >
            {saving ? 'Saving…' : 'Save'}
          </Button>
        </div>
      </div>
    </div>
  );
}
