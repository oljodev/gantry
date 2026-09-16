/** See `loop-guard.js`; the implementation is plain JavaScript so that every embedder — the
 * app, the conformance harness — can read and run the one file. */

export declare const LOOP_BUDGET_MS: number;
export declare const LOOP_GUARD_MESSAGE: string;
/** The guard's runtime, as source, for the html prelude to carry. */
export declare const LOOP_GUARD_RUNTIME: string;
/** Every inline script in an html artifact's document, with its loops guarded. */
export declare function guardHtmlScripts(html: string): string;
/** One script body, guarded — or returned as it was, when anything is unclear. */
export declare function guardScript(code: string): string;
