/**
 * The sandbox conformance artifact (docs/plan/13 §5): an `html` artifact whose script tries
 * every way out of the sandbox and reports each outcome through the bridge as console lines
 * `probe <name>: blocked|OPEN <detail>`. The app asserts that every attempt is blocked.
 *
 * The cases themselves live in `cases.json`, which is also what the WebKitGTK harness
 * (`scripts/sandbox-conformance.py`) and the automated test
 * (`desktop/crates/gantry-agent/tests/artifacts_sandbox.rs`) read, so the gallery's run by
 * hand and the run in CI are the same run.
 */

import CASES_JSON from './cases.json?raw';
import HARNESS_JS from './harness.js?raw';

export interface ConformanceCase {
  /** Stable id; the name the report and the test use. */
  id: string;
  /** The rule from 13 §5 this case holds to account. */
  rule: string;
  /** Why the rule exists, in one sentence. */
  doc: string;
  /** A CSP directive whose violation report must accompany the denial. */
  directive?: string;
  /** An engine-side signal the harness watches: `navigation`, `dialog` or `download`. */
  observer?: string;
  /** A case that is expected to hang the web process; run alone, never with the others. */
  hang?: boolean;
  /** A case only the embedder can judge; the page's own reading of it would mislead. */
  engine_only?: boolean;
  /** The body of an async function: returns 'blocked' or throws when the rule holds. */
  attempt: string;
}

interface CaseFile {
  remote: string;
  cases: ConformanceCase[];
}

const FILE = JSON.parse(CASES_JSON) as CaseFile;

/** The unreachable host the cases reach for (RFC 2606 `.invalid`, so nothing resolves). */
const REMOTE_URL = FILE.remote;

/** Every case, in document order. */
export const CASES: ConformanceCase[] = FILE.cases;

/**
 * The cases one document can run side by side and judge for itself: not the one that freezes
 * the web process, and not the ones whose verdict only the embedder sees — a page that
 * navigates itself away cannot report that it did, and would say `blocked` on its way out.
 */
const IN_THE_GALLERY = FILE.cases.filter((c) => !c.hang && !c.engine_only);

/** The names the app expects to see reported, all as `blocked`. */
export const PROBES: string[] = IN_THE_GALLERY.map((c) => c.id);

/** The case list as the driver wants it: `{ id, directive, run }`. */
function caseScript(cases: ConformanceCase[]): string {
  const one = (c: ConformanceCase) =>
    `{id:${JSON.stringify(c.id)},directive:${JSON.stringify(c.directive ?? null)},` +
    `run:async function(){${c.attempt}\n}}`;
  return `[${cases.map(one).join(',\n')}]`;
}

/**
 * One `html` artifact that runs `cases`, hands each verdict to `reportJs` — a JavaScript
 * expression for a `function (id, verdict, detail, violations)` — and calls `doneJs`, a
 * `function (violations)`, when the last one has reported.
 */
function conformanceDocument(cases: ConformanceCase[], reportJs: string, doneJs: string): string {
  const body = [
    `var REMOTE = ${JSON.stringify(REMOTE_URL)};`,
    HARNESS_JS,
    `__gantryConformance(${caseScript(cases)}, ${reportJs}, ${doneJs}, 3000);`,
  ]
    .join('\n')
    // A closing tag inside the script would end it early.
    .replace(/<\/script/gi, '<\\/script');
  return `<!doctype html>
<html><head><meta charset="utf-8"><title>Sandbox conformance</title></head>
<body style="font-family:ui-monospace,monospace;font-size:12px">
<h3>Sandbox conformance</h3><ul id="out"></ul>
<script>
${body}
</script></body></html>`;
}

/** The gallery's document: every case that is safe to run beside the others, reported to the
 * Problems pipe as `probe <id>: blocked|OPEN <detail>` and listed on the page. */
export const CONFORMANCE_HTML = conformanceDocument(
  IN_THE_GALLERY,
  `function (id, verdict, detail) {
    var li = document.createElement('li');
    li.textContent = id + ': ' + verdict + (detail ? ' (' + detail + ')' : '');
    document.getElementById('out').appendChild(li);
    console.log('probe ' + id + ': ' + verdict + (detail ? ' ' + detail : ''));
  }`,
  `function () {
    console.log('probe done');
  }`,
);
