import { createFileRoute } from '@tanstack/react-router';

import { CodeHome } from '@/features/code/CodeHome';

export const Route = createFileRoute('/code/')({
  component: CodeHome,
});
