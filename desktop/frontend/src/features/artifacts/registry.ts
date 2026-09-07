/**
 * Type → renderer, mirroring `gantry-agent/src/artifacts/registry.rs` (docs/plan/13 §3).
 * Adding a type is an entry here, one there, and a renderer.
 */

export type ArtifactType = 'markdown' | 'code' | 'svg' | 'html' | 'mermaid' | 'react';

export interface TypeInfo {
  id: ArtifactType;
  label: string;
  /** Rendered by the app itself, or inside the sandboxed iframe. */
  execution: 'none' | 'sandbox';
  /** Renders progressively while the arguments stream; sandbox types wait for the end. */
  streamsRender: boolean;
  extension: string;
}

export const TYPES: readonly TypeInfo[] = [
  { id: 'markdown', label: 'Markdown', execution: 'none', streamsRender: true, extension: 'md' },
  { id: 'code', label: 'Code', execution: 'none', streamsRender: true, extension: 'txt' },
  { id: 'svg', label: 'SVG', execution: 'none', streamsRender: false, extension: 'svg' },
  { id: 'html', label: 'HTML', execution: 'sandbox', streamsRender: false, extension: 'html' },
  { id: 'mermaid', label: 'Mermaid', execution: 'sandbox', streamsRender: false, extension: 'mmd' },
  { id: 'react', label: 'React', execution: 'sandbox', streamsRender: false, extension: 'tsx' },
];

export function typeInfo(id: string): TypeInfo | undefined {
  return TYPES.find((t) => t.id === id);
}

export function isSandboxed(id: string): boolean {
  return typeInfo(id)?.execution === 'sandbox';
}

/** The names of the runtime tools that make artifacts (13 §2). */
export const ARTIFACT_TOOLS = new Set([
  'gantry__create_artifact',
  'gantry__update_artifact',
  'gantry__edit_artifact',
]);

export function isArtifactTool(modelToolName: string): boolean {
  return ARTIFACT_TOOLS.has(modelToolName);
}

/** The shiki language for a `code` artifact, or the type's own. */
export function highlightLanguage(type: string, language: string | null | undefined): string {
  if (type === 'code') return (language ?? 'text').toLowerCase();
  switch (type) {
    case 'react':
      return language === 'jsx' ? 'jsx' : 'tsx';
    case 'html':
      return 'html';
    case 'svg':
      return 'xml';
    case 'markdown':
      return 'markdown';
    case 'mermaid':
      return 'mermaid';
    default:
      return 'text';
  }
}
