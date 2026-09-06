import { createFileRoute } from '@tanstack/react-router';

export const Route = createFileRoute('/connectors')({
  component: () => (
    <div className="mx-auto w-full max-w-(--measure) px-6 py-8">
      <h1 className="text-page font-semibold text-fg">Connectors</h1>
      <p className="mt-2 text-body text-fg-2">The catalog arrives with M9.</p>
    </div>
  ),
});
