/**
 * The loop guard for `html` artifacts (docs/plan/13 §5, hang risk 1).
 *
 * A `react` artifact is compiled, so Babel can put an elapsed-time check inside every loop
 * body (`../react/loop-guard.ts`). An `html` artifact is the document itself: its `<script>`
 * bodies are parsed by the engine the moment they are seen, and nothing inside the sandbox can
 * get between the parser and the code. So the rewrite happens on the way in, in the parent,
 * while the document is still a string — and this file is that rewrite.
 *
 * Plain JavaScript, read as text by every embedder, the way `../conformance/harness.js` is:
 *
 * - `desktop/frontend/src/features/artifacts/bridge.ts` imports it and rewrites an artifact's
 *   scripts before the document is handed to the frame;
 * - `../conformance/scripts/sandbox-conformance.py` runs this very file inside the WebKitGTK
 *   view it uses as the app's stand-in, so the conformance run is of the app's transform and
 *   not of a Python copy of it.
 *
 * **What it injects.** One call, at the head of every loop, into a runtime that answers the
 * question the hang risk is actually about: *how long has this page held on to the main
 * thread?* Not how long this loop has run — a page frozen for three seconds is frozen whether
 * one loop did it or four in a row, and the answer needs no per-loop bookkeeping, which in
 * turn means every injection is a pure insertion into a loop's head. Nothing has to find where
 * a body begins or ends, so a braceless `while (true);` is guarded exactly like a block.
 *
 * **What it will not do.** It is a mitigation, not a boundary — the sandbox is the boundary.
 * It leaves alone what it cannot rewrite safely: a script with a `src`, a module that imports
 * (neither parses on its own), an `onclick=` attribute, a `for…in` (whose key set is finite),
 * `for await`, code inside a template substitution, and anything built at runtime out of
 * strings. And it checks its own work: the rewrite is kept only if the engine parses both the
 * original and the rewrite, so a script this scanner misreads runs exactly as written.
 */

/** Three seconds, as 13 §5 states for both artifact kinds. */
export const LOOP_BUDGET_MS = 3000;

/** What a stopped loop says, so the panel, the model and Rust all recognise it. */
export const LOOP_GUARD_MESSAGE = 'Artifact loop guard';

const CHECK = 'window.__gantryLoopCheck()';
const ITER = 'window.__gantryLoopIter';

/**
 * The runtime the injected calls reach, installed by the html prelude before the artifact's
 * own scripts run.
 *
 * `yielded` is the last moment the event loop got a turn. A timer can only fire when the main
 * thread is free, so a page spinning inside a loop cannot refresh it, and the distance from it
 * is exactly how long the window has been frozen. It is reset when the guard fires, so the
 * next thing the page tries starts with a full budget rather than throwing at once.
 */
export const LOOP_GUARD_RUNTIME = `(function () {
  var budget = ${LOOP_BUDGET_MS};
  var yielded = Date.now();
  setInterval(function () { yielded = Date.now(); }, 250);
  window.__gantryLoopCheck = function () {
    if (Date.now() - yielded <= budget) return;
    yielded = Date.now();
    throw new Error(
      '${LOOP_GUARD_MESSAGE}: this page ran for more than ' +
        budget / 1000 +
        ' seconds without letting the window back in, and was stopped'
    );
  };
  window.__gantryLoopIter = function (iterable) {
    if (!iterable || typeof iterable[Symbol.iterator] !== 'function') return iterable;
    var guarded = {};
    guarded[Symbol.iterator] = function () {
      var inner = iterable[Symbol.iterator]();
      return {
        next: function (value) { window.__gantryLoopCheck(); return inner.next(value); },
        'return': function (value) {
          return inner['return'] ? inner['return'](value) : { done: true, value: value };
        },
        'throw': function (error) {
          if (inner['throw']) return inner['throw'](error);
          throw error;
        }
      };
    };
    return guarded;
  };
})();`;

/** Types a browser runs as JavaScript. Anything else in a `<script>` is data. */
const SCRIPT_TYPES = new Set([
  '',
  'module',
  'text/javascript',
  'application/javascript',
  'text/ecmascript',
  'application/ecmascript',
  'text/jscript',
]);

const SCRIPT_ELEMENT = /<script\b([^>]*)>([\s\S]*?)<\/script\s*>/gi;

/** Rewrites the loops in every inline script of an html artifact's document. */
export function guardHtmlScripts(html) {
  return html.replace(SCRIPT_ELEMENT, (whole, attributes, body) => {
    if (!runsAsScript(attributes)) return whole;
    const guarded = guardScript(body);
    if (guarded === body) return whole;
    // `[^>]*` above means the first `>` is the end of the opening tag.
    const opens = whole.indexOf('>') + 1;
    return whole.slice(0, opens) + guarded + whole.slice(opens + body.length);
  });
}

function runsAsScript(attributes) {
  if (/\bsrc\s*=/i.test(attributes)) return false;
  const declared = /\btype\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+))/i.exec(attributes);
  if (!declared) return true;
  const type = (declared[1] ?? declared[2] ?? declared[3] ?? '').trim().toLowerCase();
  return SCRIPT_TYPES.has(type.split(';')[0].trim());
}

/**
 * One script body, guarded — or returned untouched, which is the answer whenever anything at
 * all is unclear.
 */
export function guardScript(code) {
  const guarded = inject(code);
  if (guarded === null || guarded === code) return code;
  const before = parses(code);
  // No parser to check with (a policy without `'unsafe-eval'`, say): the rewrite stands, since
  // the alternative is silently dropping the guard.
  if (before === undefined) return guarded;
  // 13 §5: "scripts that do not parse run unmodified".
  if (!before) return code;
  return parses(guarded) ? guarded : code;
}

/** `true`, `false`, or `undefined` when this environment will not parse anything for us. */
function parses(code) {
  try {
    new Function(code);
    return true;
  } catch (error) {
    return error instanceof SyntaxError ? false : undefined;
  }
}

// ------------------------------------------------------------------------------ the scanner

const IDENT = /[A-Za-z0-9_$]/;
const IDENT_START = /[A-Za-z_$]/;
const SPACE = /\s/;

/** Keywords after which a `/` begins a regular expression rather than a division. */
const BEFORE_REGEX = new Set([
  'return',
  'typeof',
  'instanceof',
  'in',
  'of',
  'new',
  'delete',
  'void',
  'throw',
  'case',
  'do',
  'else',
  'yield',
  'await',
]);

/**
 * Walks the code once, keeping track of what it is inside, and collects one insertion per loop
 * it is sure of. Returns `null` the moment anything does not add up — an unterminated string,
 * a comment with no end, a head with no closing bracket — because a scanner that guesses is
 * worse than no guard at all.
 */
function inject(source) {
  const edits = [];
  /** One entry per open `{`: `true` when it opens a block rather than an object. */
  const braces = [];
  let previous = '';
  let word = '';
  let blockClosed = false;
  let i = 0;

  while (i < source.length) {
    const c = source[i];
    if (SPACE.test(c)) {
      i += 1;
      continue;
    }
    if (c === '/' && source[i + 1] === '/') {
      const end = source.indexOf('\n', i);
      i = end === -1 ? source.length : end + 1;
      continue;
    }
    if (c === '/' && source[i + 1] === '*') {
      const end = source.indexOf('*/', i + 2);
      if (end === -1) return null;
      i = end + 2;
      continue;
    }
    if (c === '"' || c === "'" || c === '`') {
      const end = c === '`' ? skipTemplate(source, i) : skipString(source, i);
      if (end === -1) return null;
      i = end;
      previous = c;
      word = '';
      blockClosed = false;
      continue;
    }
    if (c === '/' && startsRegex(previous, word, blockClosed)) {
      const end = skipRegex(source, i);
      if (end === -1) return null;
      i = end;
      previous = '/';
      word = '';
      blockClosed = false;
      continue;
    }
    if (IDENT_START.test(c)) {
      let end = i;
      while (end < source.length && IDENT.test(source[end])) end += 1;
      const found = source.slice(i, end);
      if (
        (found === 'while' || found === 'for') &&
        startsStatement(previous, word, braces, blockClosed) &&
        loopHead(source, end, found, edits) === null
      ) {
        return null;
      }
      previous = source[end - 1];
      word = found;
      blockClosed = false;
      i = end;
      continue;
    }
    if (c === '{') {
      braces.push(opensBlock(previous, word, blockClosed));
      blockClosed = false;
    } else if (c === '}') {
      blockClosed = braces.pop() ?? false;
    } else {
      blockClosed = false;
    }
    previous = c;
    word = '';
    i += 1;
  }

  if (edits.length === 0) return source;
  edits.sort((a, b) => a.at - b.at);
  let out = '';
  let from = 0;
  for (const edit of edits) {
    out += source.slice(from, edit.at) + edit.text;
    from = edit.at;
  }
  return out + source.slice(from);
}

/**
 * Whether a `while` or `for` here opens a statement, rather than naming a method or a
 * property. The cost of saying yes wrongly is a rewrite that does not parse, which
 * `guardScript` then throws away; the cost of saying no is one unguarded loop.
 */
function startsStatement(previous, word, braces, blockClosed) {
  if (word) return word === 'else' || word === 'do';
  if (previous === '') return true;
  if (previous === ';' || previous === ')' || previous === ':') return true;
  if (previous === '}') return blockClosed;
  if (previous === '{') return braces[braces.length - 1] === true;
  return false;
}

/** Whether a `{` here opens a block. A body after `=>`, `)`, `else`, `do`, `try` is one. */
function opensBlock(previous, word, blockClosed) {
  if (word) {
    return word === 'else' || word === 'do' || word === 'try' || word === 'finally';
  }
  if (previous === '>') return true; // an arrow's body: `=>` leaves `>` behind
  if (previous === '}') return blockClosed;
  return previous === '' || previous === ';' || previous === '{' || previous === ')';
}

function startsRegex(previous, word, blockClosed) {
  if (word) return BEFORE_REGEX.has(word);
  if (previous === '') return true;
  if (previous === '}') return blockClosed;
  return !')]`\'"'.includes(previous) && !IDENT.test(previous);
}

/**
 * The head of one loop, from just after the keyword. Every injection is made here, inside the
 * brackets, which is why no body ever has to be found:
 *
 * - `while (C)` and the `while (C)` of a `do`, which this sees as the same thing, become
 *   `while (check(), C)` — the comma keeps C's own value, so nothing but the timing changes;
 * - `for (I; C; U)` becomes `for (I; check(), C; U)`, and an empty C becomes `check(), true`;
 * - `for (x of it)` becomes `for (x of iter(it))`, a wrapper that checks once per item.
 *
 * `for (k in o)` is left alone: an object's keys are a finite set, so that loop ends on its
 * own. `for await` is left alone because a synchronous wrapper would break an async iterable.
 */
function loopHead(source, from, keyword, edits) {
  let at = skipTrivia(source, from);
  if (at === -1) return null;
  let asynchronous = false;
  if (keyword === 'for' && source.startsWith('await', at) && !IDENT.test(source[at + 5] ?? '')) {
    asynchronous = true;
    at = skipTrivia(source, at + 5);
    if (at === -1) return null;
  }
  // Not a loop after all (`for` is never anything else, but say so rather than assume it).
  if (source[at] !== '(') return 0;
  const head = scanHead(source, at);
  if (head === null) return null;
  if (keyword === 'while') {
    edits.push({ at: at + 1, text: `${CHECK}, ` });
    return 0;
  }
  if (asynchronous) return 0;
  if (head.semicolons.length >= 2) {
    const [first, second] = head.semicolons;
    const empty = source.slice(first + 1, second).trim() === '';
    edits.push({ at: first + 1, text: empty ? ` ${CHECK}, true` : ` ${CHECK},` });
    return 0;
  }
  if (head.of !== -1) {
    edits.push({ at: head.of, text: ` ${ITER}(` });
    edits.push({ at: head.close, text: ')' });
  }
  return 0;
}

/**
 * From the `(` of a loop head to its `)`, noting the two `;` that make it a counting loop and
 * the `of` that makes it an iterating one. Brackets, strings, templates and comments are
 * counted or skipped; anything unterminated gives up.
 */
function scanHead(source, open) {
  const semicolons = [];
  let of = -1;
  let depth = 0;
  let previous = '';
  let word = '';
  let i = open;
  while (i < source.length) {
    const c = source[i];
    if (SPACE.test(c)) {
      i += 1;
      continue;
    }
    if (c === '/' && source[i + 1] === '/') {
      const end = source.indexOf('\n', i);
      if (end === -1) return null;
      i = end + 1;
      continue;
    }
    if (c === '/' && source[i + 1] === '*') {
      const end = source.indexOf('*/', i + 2);
      if (end === -1) return null;
      i = end + 2;
      continue;
    }
    if (c === '"' || c === "'" || c === '`') {
      const end = c === '`' ? skipTemplate(source, i) : skipString(source, i);
      if (end === -1) return null;
      i = end;
      previous = c;
      word = '';
      continue;
    }
    if (c === '/' && startsRegex(previous, word, false)) {
      const end = skipRegex(source, i);
      if (end === -1) return null;
      i = end;
      previous = '/';
      word = '';
      continue;
    }
    if (IDENT_START.test(c)) {
      let end = i;
      while (end < source.length && IDENT.test(source[end])) end += 1;
      word = source.slice(i, end);
      if (word === 'of' && depth === 1 && of === -1) of = end;
      previous = source[end - 1];
      i = end;
      continue;
    }
    if (c === '(' || c === '[' || c === '{') depth += 1;
    else if (c === ')' || c === ']' || c === '}') {
      depth -= 1;
      if (depth === 0) return { close: i, semicolons, of };
    } else if (c === ';' && depth === 1) semicolons.push(i);
    previous = c;
    word = '';
    i += 1;
  }
  return null;
}

/** Whitespace and comments, from `at`. `-1` when a comment never closes. */
function skipTrivia(source, at) {
  let i = at;
  while (i < source.length) {
    const c = source[i];
    if (SPACE.test(c)) {
      i += 1;
    } else if (c === '/' && source[i + 1] === '/') {
      const end = source.indexOf('\n', i);
      if (end === -1) return source.length;
      i = end + 1;
    } else if (c === '/' && source[i + 1] === '*') {
      const end = source.indexOf('*/', i + 2);
      if (end === -1) return -1;
      i = end + 2;
    } else {
      return i;
    }
  }
  return i;
}

/** Past the closing quote of the string that starts at `at`. */
function skipString(source, at) {
  const quote = source[at];
  let i = at + 1;
  while (i < source.length) {
    const c = source[i];
    if (c === '\\') i += 2;
    else if (c === quote) return i + 1;
    else if (c === '\n') return -1;
    else i += 1;
  }
  return -1;
}

/** Past the closing backtick, substitutions and all. */
function skipTemplate(source, at) {
  let i = at + 1;
  while (i < source.length) {
    const c = source[i];
    if (c === '\\') {
      i += 2;
      continue;
    }
    if (c === '`') return i + 1;
    if (c === '$' && source[i + 1] === '{') {
      const end = skipSubstitution(source, i + 1);
      if (end === -1) return -1;
      i = end;
      continue;
    }
    i += 1;
  }
  return -1;
}

/** Past the `}` closing a `${…}`, which may hold strings, templates and braces of its own. */
function skipSubstitution(source, at) {
  let depth = 0;
  let i = at;
  while (i < source.length) {
    const c = source[i];
    if (c === '"' || c === "'" || c === '`') {
      const end = c === '`' ? skipTemplate(source, i) : skipString(source, i);
      if (end === -1) return -1;
      i = end;
      continue;
    }
    if (c === '/' && source[i + 1] === '/') {
      const end = source.indexOf('\n', i);
      if (end === -1) return -1;
      i = end + 1;
      continue;
    }
    if (c === '/' && source[i + 1] === '*') {
      const end = source.indexOf('*/', i + 2);
      if (end === -1) return -1;
      i = end + 2;
      continue;
    }
    if (c === '{') depth += 1;
    else if (c === '}') {
      depth -= 1;
      if (depth === 0) return i + 1;
    }
    i += 1;
  }
  return -1;
}

/** Past the closing `/` of a regular expression and its flags. */
function skipRegex(source, at) {
  let inClass = false;
  let i = at + 1;
  while (i < source.length) {
    const c = source[i];
    if (c === '\\') {
      i += 2;
      continue;
    }
    if (c === '\n') return -1;
    if (inClass) {
      if (c === ']') inClass = false;
    } else if (c === '[') {
      inClass = true;
    } else if (c === '/') {
      i += 1;
      while (i < source.length && IDENT.test(source[i])) i += 1;
      return i;
    }
    i += 1;
  }
  return -1;
}
