import { createFileRoute } from '@tanstack/react-router';

import { Browse } from '@/features/connectors/Browse';

export const Route = createFileRoute('/connectors')({
  component: Browse,
});
