/**
 * What a person types to find one row of a menu (docs/plan/15 §8).
 *
 * Every menu in the app is searched by typing into it: press a letter or a digit and the
 * highlight jumps to the first row that starts with it. The text matched is the row's own,
 * which is right until a row is a qualified id — `openrouter/black-forest-labs/flux.2-pro`,
 * `Anthropic: Claude Sonnet 5` — and then every row in a list of models begins with the name of
 * a company rather than the name of the thing. Nobody types the provider they are already
 * signed in to; they type `flux`, or `claude`, or `v4`.
 */
export function typeaheadLabel(text: string): string {
  // Only an id is cut at its slashes. "Read/write access" is a phrase, not a path, and its
  // first word is exactly what somebody would type to find it.
  const path = /\s/.test(text) ? text : text.slice(text.lastIndexOf('/') + 1);
  const colon = path.indexOf(': ');
  const name = (colon > 0 ? path.slice(colon + 2) : path).trim();
  // A row that is nothing but a prefix keeps what it had: better to match the whole of a
  // strange label than nothing at all.
  return name || text;
}
