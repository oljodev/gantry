import { describe, expect, it } from 'vitest'
import type { Attachment, AttachmentKind } from '../api/types'
import { deliveryHint, fileSize, isPreviewable } from './attachments'

function make(overrides: Partial<Attachment> = {}): Attachment {
  return {
    id: 'a1',
    project_id: 'p1',
    filename: 'file.txt',
    media_type: 'text/plain',
    kind: 'text',
    size_bytes: 100,
    pages: 0,
    extracted_chars: 100,
    extract_error: '',
    created_at: '2026-08-06T00:00:00Z',
    ...overrides,
  }
}

describe('fileSize', () => {
  it('scales the unit to the magnitude', () => {
    expect(fileSize(0)).toBe('0 B')
    expect(fileSize(900)).toBe('900 B')
    expect(fileSize(20_480)).toBe('20.0 KB')
    expect(fileSize(3_500_000)).toBe('3.3 MB')
  })

  it('returns nothing for a nonsensical size', () => {
    expect(fileSize(-1)).toBe('')
    expect(fileSize(Number.NaN)).toBe('')
  })
})

describe('deliveryHint', () => {
  it('says how much text a document contributes', () => {
    expect(deliveryHint(make({ kind: 'text', extracted_chars: 1234 }))).toContain('1,234')
  })

  it('reports an empty text file rather than a character count', () => {
    expect(deliveryHint(make({ kind: 'text', extracted_chars: 0 }))).toBe('empty file')
  })

  it('distinguishes a readable PDF from a scanned one', () => {
    expect(deliveryHint(make({ kind: 'pdf', pages: 3, extracted_chars: 900 }))).toContain('3 page')
    expect(deliveryHint(make({ kind: 'pdf', pages: 3, extracted_chars: 0 }))).toContain(
      'vision model',
    )
  })

  it('does not promise a capability the run may not have for images', () => {
    // The worker's model decides; the badge must not claim the image will be
    // seen directly when the run may transcribe it instead.
    const hint = deliveryHint(make({ kind: 'image', media_type: 'image/png' }))
    expect(hint).toContain('image')
    expect(hint).toContain('text-only')
  })

  it('surfaces an extraction failure above everything else', () => {
    const hint = deliveryHint(
      make({ kind: 'pdf', extract_error: 'the PDF is password-protected' }),
    )
    expect(hint).toContain('password-protected')
  })

  it('warns that audio and video need native support', () => {
    for (const kind of ['audio', 'video'] as AttachmentKind[]) {
      expect(deliveryHint(make({ kind }))).toContain('native support')
    }
  })
})

describe('isPreviewable', () => {
  it('is true only for formats a browser renders inline', () => {
    expect(isPreviewable('image', 'image/png')).toBe(true)
    expect(isPreviewable('image', 'image/webp')).toBe(true)
    expect(isPreviewable('image', 'image/tiff')).toBe(false)
    expect(isPreviewable('pdf', 'application/pdf')).toBe(false)
    expect(isPreviewable('text', 'image/svg+xml')).toBe(false)
  })
})
