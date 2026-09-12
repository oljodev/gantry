/**
 * `/skill-name` in the composer (docs/plan/12 §A4 rule 5).
 *
 * A skill named this way is injected whatever it scores and whatever the last six turns
 * carried, because the user asked for it by name. The text is sent as typed: the `/name` stays
 * in the message, so the transcript shows what was actually sent rather than a cleaned-up
 * version of it, and the model reads the request the user wrote.
 */

/** The `/name` tokens at the start of a message, in order, without repeats. */
export function invokedSkills(text: string, known: string[]): string[] {
  const out: string[] = [];
  for (const token of text.trimStart().split(/\s+/)) {
    if (!token.startsWith('/')) break;
    const name = token.slice(1).toLowerCase();
    if (known.includes(name) && !out.includes(name)) out.push(name);
  }
  return out;
}

/**
 * The partial `/name` the caret is inside, or `null` when the menu should not be open.
 *
 * Only at the start of the message, and only while the token is still being typed: a `/` in
 * the middle of a sentence is a slash, and one already followed by a space is a decision the
 * user has finished making.
 */
export function slashQuery(text: string, caret: number): string | null {
  const before = text.slice(0, caret);
  const match = /(?:^|^(?:\/[a-z0-9-]+\s+)+)\/([a-z0-9-]*)$/.exec(before);
  return match?.[1] ?? null;
}

/** Replaces the partial `/name` the caret is in with the chosen one, and a trailing space. */
export function completeSlash(text: string, caret: number, name: string): [string, number] {
  const before = text.slice(0, caret);
  const start = before.lastIndexOf('/');
  if (start < 0) return [text, caret];
  const next = `${before.slice(0, start)}/${name} `;
  return [next + text.slice(caret), next.length];
}
