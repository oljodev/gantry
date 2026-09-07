import { Markdown } from '@/components/gantry/markdown/Markdown';

/** The chat's Markdown pipeline (react-markdown, no raw HTML); streams block by block (13 §3). */
export function MarkdownRenderer({ content }: { content: string }) {
  return (
    <div className="px-5 py-4">
      <Markdown className="text-body">{content}</Markdown>
    </div>
  );
}
