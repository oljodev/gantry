/**
 * Reads the string fields of a JSON object that is still streaming in (docs/plan/13 §2, 05
 * §4): `{"type":"react","title":"Dash","content":"import Re…` yields the values seen so far,
 * the last one unterminated. Only top-level string fields are extracted; that is what the
 * artifact tools' arguments are.
 */

export function partialStrings(prefix: string): Record<string, string> {
  const out: Record<string, string> = {};
  let i = 0;
  const n = prefix.length;
  // Skip to the object's first key.
  while (i < n && prefix[i] !== '{') i++;
  i++;
  while (i < n) {
    // key
    while (i < n && /[\s,]/.test(prefix[i]!)) i++;
    if (i >= n || prefix[i] === '}') break;
    if (prefix[i] !== '"') break;
    const key = readString(prefix, i + 1);
    if (key === null) break;
    i = key.end;
    while (i < n && /\s/.test(prefix[i]!)) i++;
    if (prefix[i] !== ':') break;
    i++;
    while (i < n && /\s/.test(prefix[i]!)) i++;
    if (i >= n) break;
    if (prefix[i] === '"') {
      const value = readString(prefix, i + 1);
      if (value === null) {
        // Unterminated: everything after the quote, unescaped as far as it goes.
        out[key.text] = unescape(prefix.slice(i + 1));
        break;
      }
      out[key.text] = value.text;
      i = value.end;
    } else {
      // A non-string value: skip it (numbers, booleans, null, nested objects or arrays).
      const skipped = skipValue(prefix, i);
      if (skipped === null) break;
      i = skipped;
    }
  }
  return out;
}

function readString(s: string, start: number): { text: string; end: number } | null {
  let i = start;
  let raw = '';
  while (i < s.length) {
    const c = s[i]!;
    if (c === '\\') {
      if (i + 1 >= s.length) return null;
      raw += c + s[i + 1];
      i += 2;
      continue;
    }
    if (c === '"') return { text: unescape(raw), end: i + 1 };
    raw += c;
    i++;
  }
  return null;
}

function skipValue(s: string, start: number): number | null {
  let depth = 0;
  let i = start;
  let inString = false;
  while (i < s.length) {
    const c = s[i]!;
    if (inString) {
      if (c === '\\') i++;
      else if (c === '"') inString = false;
    } else if (c === '"') inString = true;
    else if (c === '{' || c === '[') depth++;
    else if (c === '}' || c === ']') {
      if (depth === 0) return i;
      depth--;
    } else if (c === ',' && depth === 0) return i;
    i++;
  }
  return depth === 0 && !inString ? i : null;
}

/** JSON string escapes, tolerant of a truncated escape at the very end. */
function unescape(raw: string): string {
  let out = '';
  let i = 0;
  while (i < raw.length) {
    const c = raw[i]!;
    if (c !== '\\') {
      out += c;
      i++;
      continue;
    }
    const next = raw[i + 1];
    if (next === undefined) break;
    switch (next) {
      case 'n':
        out += '\n';
        break;
      case 't':
        out += '\t';
        break;
      case 'r':
        out += '\r';
        break;
      case 'b':
        out += '\b';
        break;
      case 'f':
        out += '\f';
        break;
      case 'u': {
        const hex = raw.slice(i + 2, i + 6);
        if (hex.length < 4) return out;
        out += String.fromCharCode(parseInt(hex, 16));
        i += 4;
        break;
      }
      default:
        out += next;
    }
    i += 2;
  }
  return out;
}
