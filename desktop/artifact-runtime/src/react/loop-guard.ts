/**
 * A Babel plugin that injects an elapsed-time guard into every `for`, `while` and `do` body
 * (docs/plan/13 §5): a loop running longer than the budget throws `Artifact loop guard`,
 * which surfaces as a runtime error instead of freezing the window.
 */

export const LOOP_BUDGET_MS = 3000;

/** The guard's runtime, installed once on `window` before compiled code runs. */
export function installLoopGuardRuntime(budgetMs = LOOP_BUDGET_MS) {
  const w = window as unknown as {
    __gantryLoopStart?: (id: number) => number;
    __gantryLoopCheck?: (id: number, start: number) => void;
  };
  w.__gantryLoopStart = () => Date.now();
  w.__gantryLoopCheck = (_id: number, start: number) => {
    if (Date.now() - start > budgetMs) {
      throw new Error(
        `Artifact loop guard: a loop ran for more than ${budgetMs / 1000} seconds and was stopped`,
      );
    }
  };
}

interface Types {
  identifier: (name: string) => unknown;
  numericLiteral: (n: number) => unknown;
  callExpression: (callee: unknown, args: unknown[]) => unknown;
  memberExpression: (obj: unknown, prop: unknown) => unknown;
  variableDeclaration: (kind: string, decls: unknown[]) => unknown;
  variableDeclarator: (id: unknown, init: unknown) => unknown;
  expressionStatement: (e: unknown) => unknown;
  blockStatement: (body: unknown[]) => unknown;
  isBlockStatement: (n: unknown) => boolean;
}

/** `babel.registerPlugin('gantry-loop-guard', loopGuardPlugin)`. */
export function loopGuardPlugin({ types: t }: { types: Types }) {
  let counter = 0;
  const guard = (path: {
    node: { body: unknown };
    insertBefore: (n: unknown) => void;
    scope: { generateUidIdentifier: (hint: string) => { name: string } };
  }) => {
    const id = counter++;
    const start = path.scope.generateUidIdentifier('loopStart');
    path.insertBefore(
      t.variableDeclaration('const', [
        t.variableDeclarator(
          t.identifier(start.name),
          t.callExpression(
            t.memberExpression(t.identifier('window'), t.identifier('__gantryLoopStart')),
            [t.numericLiteral(id)],
          ),
        ),
      ]),
    );
    const check = t.expressionStatement(
      t.callExpression(
        t.memberExpression(t.identifier('window'), t.identifier('__gantryLoopCheck')),
        [t.numericLiteral(id), t.identifier(start.name)],
      ),
    );
    const body = path.node.body as { body?: unknown[] };
    if (t.isBlockStatement(body) && Array.isArray(body.body)) {
      body.body.unshift(check);
    } else {
      path.node.body = t.blockStatement([check, path.node.body]);
    }
  };
  return {
    name: 'gantry-loop-guard',
    visitor: {
      ForStatement: guard,
      ForInStatement: guard,
      ForOfStatement: guard,
      WhileStatement: guard,
      DoWhileStatement: guard,
    },
  };
}
