import { createFileRoute } from '@tanstack/react-router';

import { Onboarding } from '@/features/onboarding/Onboarding';

export const Route = createFileRoute('/onboarding')({
  component: Onboarding,
});
