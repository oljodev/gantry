import { useRef, useState } from 'react'
import { File, FileText, Film, Image, Music, Paperclip, X } from 'lucide-react'
import { attachmentContentUrl, deleteAttachment, uploadAttachment } from '../api/client'
import type { Attachment, AttachmentKind } from '../api/types'
import { deliveryHint, fileSize, isPreviewable } from '../lib/attachments'

const KIND_ICONS: Record<AttachmentKind, typeof File> = {
  image: Image,
  pdf: FileText,
  text: FileText,
  audio: Music,
  video: Film,
  other: File,
}

/**
 * Attach files to a prompt before launching.
 *
 * Uploads happen immediately (so extraction and classification are done, and
 * the badge can tell the truth about what the agent will receive) and the
 * launch only carries the resulting ids. Removing a badge deletes the upload —
 * nothing is left dangling from an abandoned launch form.
 */
export function AttachmentPicker({
  attachments,
  onChange,
  projectId,
}: {
  attachments: Attachment[]
  onChange: (next: Attachment[]) => void
  projectId?: string
}) {
  const input = useRef<HTMLInputElement>(null)
  const [busy, setBusy] = useState(0)
  const [error, setError] = useState<string | null>(null)
  const [dragging, setDragging] = useState(false)

  const add = async (files: FileList | null) => {
    if (!files?.length) return
    setError(null)
    setBusy((n) => n + files.length)
    const uploaded: Attachment[] = []
    for (const file of Array.from(files)) {
      try {
        uploaded.push(await uploadAttachment(file, projectId))
      } catch (err) {
        setError(`${file.name}: ${String(err)}`)
      } finally {
        setBusy((n) => n - 1)
      }
    }
    if (uploaded.length) onChange([...attachments, ...uploaded])
  }

  const remove = async (attachment: Attachment) => {
    onChange(attachments.filter((a) => a.id !== attachment.id))
    // Best effort: the launch already dropped it, so a failed cleanup only
    // leaves an orphaned blob, never a wrong prompt.
    await deleteAttachment(attachment.id).catch(() => undefined)
  }

  return (
    <div
      className={`flex flex-col gap-2 rounded-lg border border-dashed p-2 transition ${
        dragging ? 'border-amber-600 bg-amber-950/20' : 'border-zinc-800'
      }`}
      onDragOver={(e) => {
        e.preventDefault()
        setDragging(true)
      }}
      onDragLeave={() => setDragging(false)}
      onDrop={(e) => {
        e.preventDefault()
        setDragging(false)
        void add(e.dataTransfer.files)
      }}
    >
      <div className="flex items-center gap-3">
        <input
          ref={input}
          type="file"
          multiple
          className="hidden"
          onChange={(e) => {
            void add(e.target.files)
            e.target.value = ''
          }}
        />
        <button
          type="button"
          onClick={() => input.current?.click()}
          className="flex items-center gap-1.5 rounded-md border border-zinc-700 px-2.5 py-1 text-xs text-zinc-300 transition hover:bg-zinc-900"
        >
          <Paperclip className="h-3.5 w-3.5" aria-hidden />
          Attach files
        </button>
        <span className="text-[11px] text-zinc-600">
          {busy > 0
            ? `uploading ${busy} file(s)…`
            : 'images, PDFs, code and text — drop them here or browse'}
        </span>
      </div>

      {attachments.length > 0 && (
        <ul className="flex flex-wrap gap-2" aria-label="Attached files">
          {attachments.map((attachment) => (
            <AttachmentBadge
              key={attachment.id}
              attachment={attachment}
              onRemove={() => void remove(attachment)}
            />
          ))}
        </ul>
      )}

      {error && <span className="text-xs text-red-400">{error}</span>}
    </div>
  )
}

function AttachmentBadge({
  attachment,
  onRemove,
}: {
  attachment: Attachment
  onRemove: () => void
}) {
  const Icon = KIND_ICONS[attachment.kind] ?? File
  const hint = deliveryHint(attachment)
  const preview = isPreviewable(attachment.kind, attachment.media_type)
  return (
    <li
      title={`${attachment.media_type} — ${hint}`}
      className="flex max-w-72 items-center gap-2 rounded-md border border-zinc-800 bg-zinc-900/60 py-1 pl-1 pr-1.5"
    >
      {preview ? (
        <img
          src={attachmentContentUrl(attachment.id)}
          alt=""
          className="h-8 w-8 shrink-0 rounded object-cover"
        />
      ) : (
        <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded bg-zinc-800">
          <Icon className="h-4 w-4 text-zinc-400" aria-hidden />
        </span>
      )}
      <span className="min-w-0 flex-1">
        <span className="block truncate text-xs text-zinc-200">{attachment.filename}</span>
        <span className="block truncate text-[10px] text-zinc-500">
          {fileSize(attachment.size_bytes)}
          {attachment.extract_error ? ' — unreadable' : ` — ${hint}`}
        </span>
      </span>
      <button
        type="button"
        onClick={onRemove}
        aria-label={`Remove ${attachment.filename}`}
        className="rounded p-0.5 text-zinc-600 transition hover:bg-zinc-800 hover:text-zinc-300"
      >
        <X className="h-3.5 w-3.5" aria-hidden />
      </button>
    </li>
  )
}
