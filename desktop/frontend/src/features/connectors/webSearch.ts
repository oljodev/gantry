import type { ConnectorInstanceDto } from '@/bindings';

/**
 * Where "Web search" comes from for a model whose provider has none (docs/plan/02 §3, 03 §11).
 *
 * Provider-side search is one round trip inside the provider and costs nothing to set up, so
 * it wins wherever the model has it. Where it does not, the composer's switch used to be
 * greyed out with "Not on this model" — a true sentence about a thing the app could do
 * perfectly well, because the `web` connector is first-party, native, keyless and a singleton:
 * turning it on is an install and an attach, with nothing to sign up for and nothing to pay.
 */
export const WEB_CONNECTOR = 'web';

/** The installed `web` instance, if this machine has one yet. */
export function webInstance(
  installed: ConnectorInstanceDto[] | undefined,
): ConnectorInstanceDto | null {
  return (installed ?? []).find((c) => c.catalog_id === WEB_CONNECTOR) ?? null;
}
