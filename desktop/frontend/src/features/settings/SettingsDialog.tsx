import {
  DatabaseIcon,
  GearIcon,
  InfoIcon,
  KeyIcon,
  PaletteIcon,
  PuzzlePieceIcon,
  ShieldCheckIcon,
  SlidersHorizontalIcon,
} from '@phosphor-icons/react';
import { useState } from 'react';

import {
  PrefsDialog,
  RailLink,
  SectionTitle,
  type RailItem,
} from '@/components/gantry/settings/PrefsDialog';
import { SECTIONS, type Section } from '@/features/settings/sections';
import { SettingsBody } from '@/features/settings/SettingsBody';
import { useUiStore } from '@/lib/stores/uiStore';

const ICONS: Record<Section, RailItem<Section>['icon']> = {
  general: GearIcon,
  appearance: PaletteIcon,
  providers: KeyIcon,
  guard: ShieldCheckIcon,
  data: DatabaseIcon,
  advanced: SlidersHorizontalIcon,
  about: InfoIcon,
};

/**
 * What a section is about, for the rail's search box. The words are the ones people type when
 * they cannot remember which section holds a setting.
 */
const KEYWORDS: Record<Section, string> = {
  general: 'mode guard instructions artifacts suggestions defaults',
  appearance: 'theme dark light density font colours',
  providers: 'api key openrouter anthropic openai gemini xai model models pricing endpoint',
  guard: 'judge guardrails blocked decisions sensitive paths',
  data: 'directory export backup database privacy telemetry vacuum',
  advanced: 'tokens reply length tool rounds developer prompt logs secret store',
  about: 'version licence update platform',
};

export function SettingsDialog() {
  const section = useUiStore((s) => s.settings);
  const close = useUiStore((s) => s.closeSettings);
  const open = useUiStore((s) => s.openSettings);
  const openCustomize = useUiStore((s) => s.openCustomize);
  const [query, setQuery] = useState('');

  const q = query.trim().toLowerCase();
  const items: RailItem<Section>[] = SECTIONS.filter(
    ([id, label]) =>
      !q || label.toLowerCase().includes(q) || KEYWORDS[id].includes(q) || id.includes(q),
  ).map(([id, label]) => ({ id, label, icon: ICONS[id] }));
  const active = section ?? 'general';
  const label = SECTIONS.find(([id]) => id === active)?.[1] ?? 'Settings';

  return (
    <PrefsDialog
      open={section !== null}
      onClose={close}
      title="Settings"
      items={items}
      active={active}
      onSelect={open}
      search={query}
      onSearch={setQuery}
      footer={
        <RailLink
          icon={PuzzlePieceIcon}
          label="Customize"
          onClick={() => openCustomize('connectors')}
        />
      }
    >
      {items.length === 0 ? (
        <p className="text-body text-fg-2">No settings match “{query}”.</p>
      ) : (
        <>
          <SectionTitle>{label}</SectionTitle>
          <SettingsBody section={active} />
        </>
      )}
    </PrefsDialog>
  );
}
