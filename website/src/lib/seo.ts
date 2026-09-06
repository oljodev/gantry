export const SITE_NAME = 'Gantry';
export const SITE_URL = 'https://oljo.dev';
export const REPO = 'oljodev/gantry';
export const REPO_URL = `https://github.com/${REPO}`;
export const DEFAULT_DESCRIPTION = 'Gantry is a desktop AI workspace: a chat client, a coding agent and connectors to the tools you use, running on your machine with your own API keys. macOS, Windows and Linux.';

export function pageTitle(title?: string): string {
  return title ? `${title} · ${SITE_NAME}` : `${SITE_NAME}: the AI workspace that stays on your machine`;
}
