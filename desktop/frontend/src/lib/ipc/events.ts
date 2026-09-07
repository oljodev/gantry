import { useQueryClient } from '@tanstack/react-query';
import { useEffect } from 'react';

import { events } from '@/bindings';
import { isTauri } from '@/lib/ipc/client';
import { keys } from '@/lib/ipc/keys';

/** Global backend events only say "re-fetch this" (docs/plan/01 §4). */
export function useBackendEvents() {
  const qc = useQueryClient();
  useEffect(() => {
    if (!isTauri()) return;
    const unlisten = [
      events.chatsChanged.listen((e) => {
        void qc.invalidateQueries({ queryKey: keys.chats });
        for (const id of e.payload.chat_ids) {
          void qc.invalidateQueries({ queryKey: keys.chat(id) });
        }
      }),
      events.providersChanged.listen(() => {
        void qc.invalidateQueries({ queryKey: keys.providers });
        void qc.invalidateQueries({ queryKey: ['models'] });
      }),
      events.settingsChanged.listen(() => {
        void qc.invalidateQueries({ queryKey: keys.settings });
      }),
      events.interactionsChanged.listen(() => {
        void qc.invalidateQueries({ queryKey: keys.pendingInteractions });
      }),
      events.connectorsChanged.listen(() => {
        void qc.invalidateQueries({ queryKey: keys.connectors });
        void qc.invalidateQueries({ queryKey: keys.catalog });
        void qc.invalidateQueries({ queryKey: ['chat_connectors'] });
      }),
      events.artifactsChanged.listen((e) => {
        void qc.invalidateQueries({ queryKey: ['artifact', e.payload.artifact_id] });
        void qc.invalidateQueries({ queryKey: ['artifacts'] });
        void qc.invalidateQueries({ queryKey: keys.chat(e.payload.chat_id) });
      }),
    ];
    return () => {
      for (const p of unlisten) void p.then((f) => f());
    };
  }, [qc]);
}
