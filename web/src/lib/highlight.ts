// A cheap, zero-dependency syntax highlighter for read-only code display in
// the trace. Not a full grammar per language — a generic tokenizer that colors
// the categories that carry most of the "IDE look" (comments, strings,
// numbers, keywords, constants) and adapts its comment/string style to the
// file's extension. Pure and line-oriented so the caller can render a
// line-number gutter; block comments and Python triple-strings carry state
// across lines.

export type TokenKind = 'comment' | 'string' | 'number' | 'keyword' | 'constant' | 'plain'

export interface Token {
  text: string
  kind: TokenKind
}

export interface Flavor {
  /** Line-comment prefixes, e.g. two slashes or a hash. */
  line: string[]
  /** Block-comment open/close delimiters (C-style pair). */
  block?: [string, string]
  /** Triple-quote string delimiters (Python), the two triple-quote forms. */
  triple: string[]
}

const C_LIKE: Flavor = { line: ['//'], block: ['/*', '*/'], triple: [] }
const HASH: Flavor = { line: ['#'], triple: [] }
const GENERIC: Flavor = { line: ['//', '#'], block: ['/*', '*/'], triple: [] }

const BY_EXT: Record<string, Flavor> = {
  py: { line: ['#'], triple: ['"""', "'''"] },
  pyi: { line: ['#'], triple: ['"""', "'''"] },
  js: C_LIKE,
  jsx: C_LIKE,
  ts: C_LIKE,
  tsx: C_LIKE,
  mjs: C_LIKE,
  cjs: C_LIKE,
  go: C_LIKE,
  rs: C_LIKE,
  java: C_LIKE,
  kt: C_LIKE,
  swift: C_LIKE,
  scala: C_LIKE,
  c: C_LIKE,
  h: C_LIKE,
  cpp: C_LIKE,
  hpp: C_LIKE,
  cc: C_LIKE,
  cs: C_LIKE,
  php: C_LIKE,
  css: { line: [], block: ['/*', '*/'], triple: [] },
  scss: C_LIKE,
  less: C_LIKE,
  sql: { line: ['--'], block: ['/*', '*/'], triple: [] },
  rb: HASH,
  sh: HASH,
  bash: HASH,
  zsh: HASH,
  yaml: HASH,
  yml: HASH,
  toml: HASH,
  ini: HASH,
  conf: HASH,
  dockerfile: HASH,
  json: { line: [], triple: [] },
  jsonc: C_LIKE,
  md: { line: [], triple: [] },
  markdown: { line: [], triple: [] },
  txt: { line: [], triple: [] },
}

/** Pick a highlighting flavor from a workspace-relative file path. */
export function flavorForPath(path: string): Flavor {
  const base = path.split('/').pop() ?? path
  if (base.toLowerCase() === 'dockerfile' || base.toLowerCase() === 'makefile') return HASH
  const ext = base.includes('.') ? base.split('.').pop()!.toLowerCase() : ''
  return BY_EXT[ext] ?? GENERIC
}

// A broad union of keywords across common languages — good enough to read as an
// IDE without per-language grammars.
const KEYWORDS = new Set(
  (
    'if else elif for while do switch case default break continue return yield goto match when ' +
    'function func def fn lambda class struct enum interface type trait impl module namespace ' +
    'package import from export require use using include public private protected static final ' +
    'abstract virtual override async await var let const val mutable new delete try catch finally ' +
    'throw throws raise except with as in of is not and or pass global nonlocal del assert extends ' +
    'implements super this self typeof instanceof void where defer select go chan map range ' +
    'int float double bool boolean string str char byte long short unsigned signed any unknown never'
  ).split(' '),
)

const CONSTANTS = new Set([
  'true',
  'false',
  'null',
  'undefined',
  'None',
  'True',
  'False',
  'nil',
  'NaN',
  'Infinity',
  'self',
  'this',
])

const IDENT = /[A-Za-z_$][A-Za-z0-9_$]*/y
const NUMBER = /0[xXbBoO][0-9a-fA-F_]+|\d[\d_]*\.?\d*(?:[eE][+-]?\d+)?/y

interface Carry {
  block: boolean
  triple: string | null
}

/** Highlight full text into per-line token arrays (state carries across lines
 *  for block comments and triple-quoted strings). */
export function highlightText(text: string, flavor: Flavor): Token[][] {
  const carry: Carry = { block: false, triple: null }
  return text.split('\n').map((line) => tokenizeLine(line, flavor, carry))
}

function tokenizeLine(line: string, flavor: Flavor, carry: Carry): Token[] {
  const out: Token[] = []
  let i = 0
  const push = (kind: TokenKind, text: string) => {
    if (!text) return
    const last = out[out.length - 1]
    if (last && last.kind === kind) last.text += text
    else out.push({ kind, text })
  }

  while (i < line.length) {
    if (carry.block && flavor.block) {
      const end = line.indexOf(flavor.block[1], i)
      if (end === -1) return push('comment', line.slice(i)), out
      push('comment', line.slice(i, end + flavor.block[1].length))
      i = end + flavor.block[1].length
      carry.block = false
      continue
    }
    if (carry.triple) {
      const end = line.indexOf(carry.triple, i)
      if (end === -1) return push('string', line.slice(i)), out
      push('string', line.slice(i, end + carry.triple.length))
      i = end + carry.triple.length
      carry.triple = null
      continue
    }

    const lineComment = flavor.line.find((p) => line.startsWith(p, i))
    if (lineComment) return push('comment', line.slice(i)), out

    const triple = flavor.triple.find((t) => line.startsWith(t, i))
    if (triple) {
      const end = line.indexOf(triple, i + triple.length)
      if (end === -1) {
        push('string', line.slice(i))
        carry.triple = triple
        return out
      }
      push('string', line.slice(i, end + triple.length))
      i = end + triple.length
      continue
    }

    if (flavor.block && line.startsWith(flavor.block[0], i)) {
      const end = line.indexOf(flavor.block[1], i + flavor.block[0].length)
      if (end === -1) {
        push('comment', line.slice(i))
        carry.block = true
        return out
      }
      push('comment', line.slice(i, end + flavor.block[1].length))
      i = end + flavor.block[1].length
      continue
    }

    const ch = line[i]
    if (ch === '"' || ch === "'" || ch === '`') {
      i = scanString(line, i, ch, push)
      continue
    }

    NUMBER.lastIndex = i
    const num = NUMBER.exec(line)
    if (num && num.index === i && /[0-9]/.test(ch)) {
      push('number', num[0])
      i += num[0].length
      continue
    }

    IDENT.lastIndex = i
    const id = IDENT.exec(line)
    if (id && id.index === i) {
      const word = id[0]
      push(CONSTANTS.has(word) ? 'constant' : KEYWORDS.has(word) ? 'keyword' : 'plain', word)
      i += word.length
      continue
    }

    push('plain', ch)
    i += 1
  }
  return out
}

function scanString(
  line: string,
  start: number,
  quote: string,
  push: (kind: TokenKind, text: string) => void,
): number {
  let j = start + 1
  while (j < line.length) {
    if (line[j] === '\\') {
      j += 2
      continue
    }
    if (line[j] === quote) {
      j += 1
      break
    }
    j += 1
  }
  push('string', line.slice(start, j))
  return j
}
