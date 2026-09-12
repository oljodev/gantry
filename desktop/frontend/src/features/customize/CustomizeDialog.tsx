import { BrainIcon, GearIcon, GraduationCapIcon, PuzzlePieceIcon } from '@phosphor-icons/react';

import { PrefsDialog, RailLink, type RailItem } from '@/components/gantry/settings/PrefsDialog';
import { ConnectorsSection } from '@/features/customize/ConnectorsSection';
import { MemorySection } from '@/features/customize/MemorySection';
import { SkillsSection } from '@/features/customize/SkillsSection';
import { CUSTOMIZE_SECTIONS, type CustomizeSection } from '@/features/settings/sections';
import { useUiStore } from '@/lib/stores/uiStore';

const ICONS: Record<CustomizeSection, RailItem<CustomizeSection>['icon']> = {
  connectors: PuzzlePieceIcon,
  skills: GraduationCapIcon,
  memory: BrainIcon,
};

/** What you add to Gantry, as its own dialog beside Settings (11 §2, 15 A18). */
export function CustomizeDialog() {
  const section = useUiStore((s) => s.customize);
  const close = useUiStore((s) => s.closeCustomize);
  const open = useUiStore((s) => s.openCustomize);
  const openSettings = useUiStore((s) => s.openSettings);
  const active = section ?? 'connectors';

  return (
    <PrefsDialog
      open={section !== null}
      onClose={close}
      title="Customize"
      items={CUSTOMIZE_SECTIONS.map(([id, label]) => ({ id, label, icon: ICONS[id] }))}
      active={active}
      onSelect={open}
      railHeader={
        <div className="px-2 pb-1 text-micro font-medium uppercase tracking-[0.04em] text-fg-3">
          Customize
        </div>
      }
      footer={<RailLink icon={GearIcon} label="Settings" onClick={() => openSettings('general')} />}
    >
      {active === 'connectors' && <ConnectorsSection />}
      {active === 'skills' && <SkillsSection />}
      {active === 'memory' && <MemorySection />}
    </PrefsDialog>
  );
}
