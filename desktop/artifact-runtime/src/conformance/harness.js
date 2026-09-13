/**
 * The driver the conformance cases run under (docs/plan/13 §5). Plain JavaScript, read as
 * text by every embedder: the gallery entry (probe.ts) and the WebKitGTK harness
 * (scripts/sandbox-conformance.py). It runs inside the sandboxed document, so it may use
 * nothing the sandbox does not already have.
 *
 * The embedder defines `REMOTE` and a `cases` array of `{ id, directive?, run }`, then calls
 * `__gantryConformance(cases, report, done, budgetMs)`.
 *
 * A case reports that the rule holds by returning the string 'blocked' or by throwing. Any
 * other return value is the detail of an escape. A case that never settles within its budget
 * counts as blocked: nothing opened, nothing loaded, nothing came back.
 *
 * When a case names a CSP `directive`, being denied is not enough on its own — the remote
 * hosts the cases reach for are under the reserved .invalid TLD and would fail to resolve
 * even with no policy at all. The document must also have reported a securitypolicyviolation
 * naming that directive, which is the only evidence that it was the policy that said no.
 */
(function () {
  var violations = [];
  document.addEventListener('securitypolicyviolation', function (e) {
    // CSP 3 splits `script-src` into `script-src-elem` and `script-src-attr`, so the directive
    // a report names can be narrower than the one the policy was written with. Keep both.
    var effective = String(e.effectiveDirective || '').split(' ')[0];
    var violated = String(e.violatedDirective || '').split(' ')[0];
    if (effective) violations.push(effective);
    if (violated && violated !== effective) violations.push(violated);
  });

  /** `script-src` is satisfied by a report naming `script-src-elem`. */
  function names(seen, directive) {
    for (var i = 0; i < seen.length; i++) {
      if (seen[i] === directive || seen[i].indexOf(directive + '-') === 0) return true;
    }
    return false;
  }

  function detail(e) {
    var m = e && e.message ? e.message : String(e);
    return m.length > 140 ? m.slice(0, 140) + '…' : m;
  }

  /** A violation report can arrive a tick after the operation it refused; give it one. */
  var SETTLE_MS = 150;

  window.__gantryConformance = function (cases, report, done, budgetMs) {
    var budget = budgetMs || 3000;
    var index = 0;

    function next() {
      if (index >= cases.length) {
        if (done) done(violations.slice());
        return;
      }
      var c = cases[index++];
      var mark = violations.length;
      var settled = false;

      function settle(verdict, text) {
        if (settled) return;
        settled = true;
        setTimeout(function () {
          var seen = violations.slice(mark);
          if (verdict === 'blocked' && c.directive && !names(seen, c.directive)) {
            verdict = 'OPEN';
            text =
              'denied, but the document reported no securitypolicyviolation for ' +
              c.directive +
              (seen.length ? ' (it reported ' + seen.join(', ') + ')' : '');
          }
          report(c.id, verdict, text || '', seen);
          setTimeout(next, 10);
        }, SETTLE_MS);
      }

      var started;
      try {
        started = Promise.resolve(c.run());
      } catch (e) {
        settle('blocked', detail(e));
        return;
      }
      var timer = setTimeout(function () {
        settle('blocked', 'nothing came back within ' + budget + 'ms');
      }, budget);
      started.then(
        function (r) {
          clearTimeout(timer);
          settle(r === 'blocked' ? 'blocked' : 'OPEN', r === 'blocked' ? '' : String(r));
        },
        function (e) {
          clearTimeout(timer);
          settle('blocked', detail(e));
        },
      );
    }

    next();
  };
})();
