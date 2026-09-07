import { useState } from 'react';

import type { ExportFormat } from '@/bindings';
import { SettingsGroup, SettingsRow } from '@/components/gantry/settings/SettingsRow';
import { Button } from '@/components/ui/button';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { toast } from '@/components/ui/toast';
import { isTauri } from '@/lib/ipc/client';
import { useChatMutations, useChats } from '@/lib/ipc/hooks/chats';
import { useDataInfo, useDataMutations } from '@/lib/ipc/hooks/settings';

/**
 * Settings → Data & privacy (11 §2): where the data lives, export a chat, back up the database,
 * and a plain statement of what leaves the machine.
 */
export function Data() {
  const info = useDataInfo();
  const chats = useChats();
  const { exportChat } = useChatMutations();
  const { openDataDir, backup, maintain } = useDataMutations();
  const [chatId, setChatId] = useState<string>('');
  const [format, setFormat] = useState<ExportFormat>('markdown');

  if (!isTauri()) {
    return (
      <p className="text-body text-fg-2">
        Not running inside the Gantry window, so there is no data directory to show. This section
        works in the app.
      </p>
    );
  }
  const d = info.data;
  const chatRows = (chats.data ?? []).filter((c) => !c.archived);
  const selected = chatId || chatRows[0]?.id || '';
  const title = chatRows.find((c) => c.id === selected)?.title ?? 'chat';

  const doExport = async () => {
    if (!selected) return;
    const { save } = await import('@tauri-apps/plugin-dialog');
    const ext = format === 'markdown' ? 'md' : 'json';
    const path = await save({
      title: 'Export chat',
      defaultPath: `${safeName(title)}.${ext}`,
      filters: [{ name: format === 'markdown' ? 'Markdown' : 'JSON', extensions: [ext] }],
    });
    if (!path) return;
    exportChat.mutate(
      { chatId: selected, format, path },
      {
        onSuccess: () => toast.add({ title: 'Chat exported', description: path, type: 'success' }),
        onError: (err) =>
          toast.add({ title: 'Export failed', description: describe(err), type: 'error' }),
      },
    );
  };

  const doBackup = async () => {
    const { save } = await import('@tauri-apps/plugin-dialog');
    const stamp = new Date().toISOString().slice(0, 10);
    const path = await save({
      title: 'Back up the database',
      defaultPath: `gantry-backup-${stamp}.db`,
      filters: [{ name: 'SQLite database', extensions: ['db'] }],
    });
    if (!path) return;
    backup.mutate(path, {
      onSuccess: () =>
        toast.add({
          title: 'Backup written',
          description: `${path} (keys excluded)`,
          type: 'success',
        }),
      onError: (err) =>
        toast.add({ title: 'Backup failed', description: describe(err), type: 'error' }),
    });
  };

  return (
    <div className="flex flex-col gap-8">
      <SettingsGroup title="Where your data lives">
        <SettingsRow
          label="Data directory"
          hint={<code className="selectable text-mono">{d?.data_dir ?? '…'}</code>}
        >
          <Button variant="secondary" size="sm" onClick={() => openDataDir.mutate()}>
            Open
          </Button>
        </SettingsRow>
        <SettingsRow
          label="Database"
          hint={d ? `${d.chat_count} chats · ${bytes(d.database_bytes)} on disk` : '…'}
        >
          <div className="flex gap-1">
            <Button
              variant="secondary"
              size="sm"
              onClick={() => void doBackup()}
              disabled={backup.isPending}
            >
              {backup.isPending ? 'Backing up…' : 'Back up…'}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              disabled={maintain.isPending}
              onClick={() =>
                maintain.mutate(undefined, {
                  onSuccess: () => {
                    toast.add({ title: 'Database checked and compacted', type: 'success' });
                    void info.refetch();
                  },
                  onError: (err) =>
                    toast.add({ title: 'Check failed', description: describe(err), type: 'error' }),
                })
              }
            >
              {maintain.isPending ? 'Checking…' : 'Check and compact'}
            </Button>
          </div>
        </SettingsRow>
      </SettingsGroup>

      <SettingsGroup title="Export a chat">
        <SettingsRow label="Chat" hint="Markdown keeps the readable text; JSON keeps everything.">
          <div className="flex items-center gap-1">
            <Select value={selected} onValueChange={(v) => v && setChatId(v)}>
              <SelectTrigger aria-label="Chat to export" className="max-w-64">
                <SelectValue placeholder="No chats yet" />
              </SelectTrigger>
              <SelectContent>
                {chatRows.map((c) => (
                  <SelectItem key={c.id} value={c.id}>
                    {c.title}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Select value={format} onValueChange={(v) => v && setFormat(v as ExportFormat)}>
              <SelectTrigger aria-label="Format" className="min-w-28">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="markdown">Markdown</SelectItem>
                <SelectItem value="json">JSON</SelectItem>
              </SelectContent>
            </Select>
            <Button
              variant="secondary"
              size="sm"
              disabled={!selected || exportChat.isPending}
              onClick={() => void doExport()}
            >
              Export…
            </Button>
          </div>
        </SettingsRow>
      </SettingsGroup>

      <SettingsGroup title="What leaves this machine">
        <p className="py-3 text-body text-fg-2">
          Your messages, attachments and the system prompt go to the model provider you chose for
          each chat, and nowhere else. Gantry sends no telemetry and no crash reports. API keys are
          stored encrypted and are excluded from backups.
        </p>
      </SettingsGroup>
    </div>
  );
}

function bytes(n: number): string {
  if (n < 1024 * 1024) return `${Math.max(1, Math.round(n / 1024))} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

function safeName(title: string): string {
  return (
    title
      .replace(/[^\p{L}\p{N} _-]+/gu, '')
      .trim()
      .slice(0, 60) || 'chat'
  );
}

function describe(err: unknown): string {
  if (err && typeof err === 'object' && 'message' in err)
    return String((err as { message: unknown }).message);
  return String(err);
}
