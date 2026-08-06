import type { Attachment, AttachmentKind } from '../api/types'

/** 900 → "900 B", 20_480 → "20.0 KB", 3_500_000 → "3.3 MB" */
export function fileSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return ''
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

/**
 * What the agent will actually receive for this file — the honest, one-line
 * answer to "did attaching this do anything?".
 *
 * A text or PDF attachment is read directly; an image depends on the worker's
 * model, so the badge stays neutral ("sent as an image, or described for a
 * text-only model") rather than promising a capability the run may not have.
 */
export function deliveryHint(attachment: Attachment): string {
  if (attachment.extract_error) return `could not be read: ${attachment.extract_error}`
  switch (attachment.kind) {
    case 'image':
      return 'sent as an image, or auto-described for a text-only model'
    case 'pdf':
      return attachment.extracted_chars > 0
        ? `${attachment.pages} page(s) of text extracted`
        : 'no text layer — will be described by a vision model'
    case 'text':
      return attachment.extracted_chars > 0
        ? `${attachment.extracted_chars.toLocaleString()} characters of text`
        : 'empty file'
    case 'audio':
    case 'video':
      return 'only models with native support can read this'
    default:
      return 'attached; contents may not be readable'
  }
}

/** Whether the browser can render an inline thumbnail for this file. */
export function isPreviewable(kind: AttachmentKind, mediaType: string): boolean {
  return (
    kind === 'image' &&
    ['image/png', 'image/jpeg', 'image/gif', 'image/webp'].includes(mediaType)
  )
}
