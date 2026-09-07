import { createFileRoute } from '@tanstack/react-router';

import { Welcome } from '@/features/onboarding/Welcome';

export const Route = createFileRoute('/chat/')({
  component: Welcome,
});
