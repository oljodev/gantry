import type { HunkLine } from '@/fixtures/types';
import type { ChangedSpan, LineTokens, Token } from '@/lib/diff';

/**
 * One line of a diff: syntax colour where shiki has an answer, and a stronger tint over the
 * part that actually changed (docs/plan/15 A19).
 *
 * The two are applied together rather than one or the other, which is the only fiddly part
 * here: the tokens say what colour each stretch of text is, the changed span says which
 * characters to lift, and the two do not line up. So tokens are cut at the span's edges and
 * each piece keeps its own colour. Without the cut, emphasis would have to be a whole-token
 * thing, and a token is often the whole line.
 */
export function DiffLineText({
  line,
  tokens,
  span,
}: {
  line: HunkLine;
  tokens?: LineTokens | null;
  span?: ChangedSpan | null;
}) {
  const highlighted = tokens?.get(line);
  if (!highlighted || highlighted.length === 0) {
    return <>{span ? pieces(line.text, span, (t, on) => mark(t, on)) : line.text}</>;
  }
  let at = 0;
  const out: React.ReactNode[] = [];
  for (const [i, token] of highlighted.entries()) {
    const start = at;
    at += token.content.length;
    out.push(
      <span key={i} style={token.style}>
        {span ? pieces(token.content, shift(span, start), (t, on) => mark(t, on)) : token.content}
      </span>,
    );
  }
  return <>{out}</>;
}

/** The span in the coordinates of a token that starts at `offset` in the line. */
function shift(span: ChangedSpan, offset: number): ChangedSpan {
  return { start: span.start - offset, end: span.end - offset };
}

/** Splits `text` at the span's edges, handing each piece to `render` with whether it changed. */
function pieces(
  text: string,
  span: ChangedSpan,
  render: (piece: string, changed: boolean) => React.ReactNode,
): React.ReactNode[] {
  const start = Math.max(0, Math.min(span.start, text.length));
  const end = Math.max(start, Math.min(span.end, text.length));
  const out: React.ReactNode[] = [];
  if (start > 0) out.push(render(text.slice(0, start), false));
  if (end > start) out.push(render(text.slice(start, end), true));
  if (end < text.length) out.push(render(text.slice(end), false));
  return out.map((node, i) => <span key={i}>{node}</span>);
}

function mark(text: string, changed: boolean): React.ReactNode {
  if (!changed) return text;
  return <span className="rounded-1 bg-diff-word">{text}</span>;
}

export type { Token };
