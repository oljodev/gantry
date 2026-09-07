/** An SVG as an `<img>` from a data URL: it never executes scripts (13 §3). */
export function SvgRenderer({ content, title }: { content: string; title: string }) {
  const url = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(content)}`;
  return (
    <div className="flex min-h-full items-center justify-center bg-inset p-4">
      <img src={url} alt={title} className="max-h-full max-w-full" />
    </div>
  );
}
