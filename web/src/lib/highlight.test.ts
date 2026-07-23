import { describe, expect, it } from 'vitest'
import { flavorForPath, highlightText, type TokenKind } from './highlight'

// Compact assertion helper: the (kind, text) pairs for a single line.
function line(text: string, path = 'x.ts'): Array<[TokenKind, string]> {
  const [tokens] = highlightText(text, flavorForPath(path))
  return tokens.map((t) => [t.kind, t.text])
}

describe('flavorForPath', () => {
  it('maps extensions and bare filenames to comment styles', () => {
    expect(flavorForPath('a/b/mod.py').line).toEqual(['#'])
    expect(flavorForPath('src/app.ts').line).toEqual(['//'])
    expect(flavorForPath('Dockerfile').line).toEqual(['#'])
    expect(flavorForPath('data.json').line).toEqual([])
  })
})

describe('highlightText', () => {
  it('colors keywords, constants, strings and numbers', () => {
    expect(line('const x = 42')).toEqual([
      ['keyword', 'const'],
      ['plain', ' x = '],
      ['number', '42'],
    ])
    expect(line('return true')).toEqual([
      ['keyword', 'return'],
      ['plain', ' '],
      ['constant', 'true'],
    ])
    expect(line('const s = "hi \\" there"')).toContainEqual(['string', '"hi \\" there"'])
  })

  it('treats a line comment as a single comment token to end of line', () => {
    expect(line('x() // not a keyword: return')).toContainEqual([
      'comment',
      '// not a keyword: return',
    ])
    // # is a comment in python, but not in a c-like file.
    expect(line('total = 1 # count', 'calc.py')).toContainEqual(['comment', '# count'])
    expect(line('a # 1', 'app.ts')).not.toContainEqual(['comment', '# 1'])
  })

  it('does not color keywords that appear inside strings or comments', () => {
    expect(line('"return if else"')).toEqual([['string', '"return if else"']])
    expect(line('// return if else')).toEqual([['comment', '// return if else']])
  })

  it('carries a block comment across lines', () => {
    const tokens = highlightText('a\n/* c1\nc2 */ b', flavorForPath('x.ts'))
    expect(tokens[1]).toEqual([{ kind: 'comment', text: '/* c1' }])
    expect(tokens[2]).toContainEqual({ kind: 'comment', text: 'c2 */' })
    expect(tokens[2]).toContainEqual({ kind: 'plain', text: ' b' })
  })

  it('carries a python triple-quoted string across lines', () => {
    const tokens = highlightText('x = """\ndoc if\n"""', flavorForPath('m.py'))
    expect(tokens[1]).toEqual([{ kind: 'string', text: 'doc if' }]) // 'if' not a keyword here
    expect(tokens[2]).toContainEqual({ kind: 'string', text: '"""' })
  })

  it('preserves every character (highlighting is lossless)', () => {
    const src = 'def f(x):\n    return x + 1  # ok'
    const rejoined = highlightText(src, flavorForPath('m.py'))
      .map((toks) => toks.map((t) => t.text).join(''))
      .join('\n')
    expect(rejoined).toBe(src)
  })
})
