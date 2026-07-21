// Models habitually emit literal <br> tags inside Markdown. Raw HTML rendering
// stays off (model output is untrusted), so those tags would otherwise show up
// verbatim as "<br>" in the text. Translate the one tag that matters into
// something Markdown can express.

const BR = /<br\s*\/?>/gi

/**
 * Replace <br> with a real line break — except inside a GFM table row, where a
 * newline would terminate the row and shred the table. There a space is the
 * best available approximation.
 */
export function normalizeMarkdown(text: string): string {
  return text
    .split('\n')
    .map((line) => (line.trimStart().startsWith('|') ? line.replace(BR, ' ') : line.replace(BR, '\n')))
    .join('\n')
}
