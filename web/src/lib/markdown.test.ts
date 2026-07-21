import { describe, expect, it } from 'vitest'
import { normalizeMarkdown } from './markdown'

describe('normalizeMarkdown', () => {
  it('turns <br> into a line break in ordinary prose', () => {
    expect(normalizeMarkdown('one<br>two')).toBe('one\ntwo')
  })

  it('accepts the self-closing and spaced spellings, case-insensitively', () => {
    expect(normalizeMarkdown('a<br/>b<br />c<BR>d')).toBe('a\nb\nc\nd')
  })

  it('uses a space inside table rows, where a newline would break the table', () => {
    const table = ['| a | b |', '| --- | --- |', '| x | one<br>two |'].join('\n')
    expect(normalizeMarkdown(table)).toBe(
      ['| a | b |', '| --- | --- |', '| x | one two |'].join('\n'),
    )
  })

  it('detects indented table rows too', () => {
    expect(normalizeMarkdown('  | x | one<br>two |')).toBe('  | x | one two |')
  })

  it('leaves text without <br> untouched', () => {
    const text = '# Heading\n\n- item\n- item\n'
    expect(normalizeMarkdown(text)).toBe(text)
  })
})
