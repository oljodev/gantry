import { createFileRoute, notFound } from '@tanstack/react-router';

import { isSection } from '@/features/settings/sections';
import { SettingsSection } from '@/features/settings/SettingsSection';

export const Route = createFileRoute('/settings/$section')({
  beforeLoad: ({ params }) => {
    if (!isSection(params.section)) throw notFound();
  },
  component: SectionRoute,
});

function SectionRoute() {
  const { section } = Route.useParams();
  return <SettingsSection section={section} />;
}
