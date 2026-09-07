/**
 * The import rewrite (docs/plan/13 §6): `import x from "react"` becomes a `require` over the
 * bundled modules; an import outside the allowlist fails at compile time with the list.
 */

import { MODULES, availableList } from './modules';

interface Types {
  identifier: (name: string) => unknown;
  stringLiteral: (s: string) => unknown;
  callExpression: (callee: unknown, args: unknown[]) => unknown;
  memberExpression: (obj: unknown, prop: unknown, computed?: boolean) => unknown;
  variableDeclaration: (kind: string, decls: unknown[]) => unknown;
  variableDeclarator: (id: unknown, init: unknown) => unknown;
  objectPattern: (props: unknown[]) => unknown;
  objectProperty: (
    key: unknown,
    value: unknown,
    computed?: boolean,
    shorthand?: boolean,
  ) => unknown;
  isImportDefaultSpecifier: (n: unknown) => boolean;
  isImportNamespaceSpecifier: (n: unknown) => boolean;
  isIdentifier: (n: unknown) => boolean;
  exportDefaultDeclaration: (d: unknown) => unknown;
  assignmentExpression: (op: string, left: unknown, right: unknown) => unknown;
  expressionStatement: (e: unknown) => unknown;
  functionExpression: (id: unknown, params: unknown[], body: unknown) => unknown;
  classExpression: (id: unknown, superClass: unknown, body: unknown) => unknown;
  isFunctionDeclaration: (n: unknown) => boolean;
  isClassDeclaration: (n: unknown) => boolean;
  toExpression: (n: unknown) => unknown;
}

interface Specifier {
  type: string;
  local: { name: string };
  imported?: { name?: string; value?: string };
}

export class ImportError extends Error {}

/** `babel.registerPlugin('gantry-imports', importsPlugin)`. */
export function importsPlugin({ types: t }: { types: Types }) {
  const require = (source: string) =>
    t.callExpression(t.identifier('__gantryRequire'), [t.stringLiteral(source)]);
  return {
    name: 'gantry-imports',
    visitor: {
      ImportDeclaration(path: {
        node: { source: { value: string }; specifiers: Specifier[] };
        replaceWith: (n: unknown) => void;
        remove: () => void;
      }) {
        const source = path.node.source.value;
        if (!(source in MODULES)) {
          throw new ImportError(
            `Module "${source}" is not available in Gantry artifacts. Available: ${availableList()}`,
          );
        }
        const specs = path.node.specifiers;
        if (specs.length === 0) {
          path.remove();
          return;
        }
        const declarators: unknown[] = [];
        const named: unknown[] = [];
        for (const s of specs) {
          if (t.isImportDefaultSpecifier(s)) {
            declarators.push(
              t.variableDeclarator(
                t.identifier(s.local.name),
                t.memberExpression(require(source), t.identifier('default')),
              ),
            );
          } else if (t.isImportNamespaceSpecifier(s)) {
            declarators.push(t.variableDeclarator(t.identifier(s.local.name), require(source)));
          } else {
            const imported = s.imported?.name ?? s.imported?.value ?? s.local.name;
            named.push(
              t.objectProperty(
                t.identifier(imported),
                t.identifier(s.local.name),
                false,
                imported === s.local.name,
              ),
            );
          }
        }
        if (named.length > 0) {
          declarators.push(t.variableDeclarator(t.objectPattern(named), require(source)));
        }
        path.replaceWith(t.variableDeclaration('const', declarators));
      },
      ExportDefaultDeclaration(path: {
        node: { declaration: { type: string; id?: { name: string } | null } };
        replaceWith: (n: unknown) => void;
      }) {
        // `export default X` → `__gantryExports.default = X` (declarations keep their name).
        const d = path.node.declaration;
        const target = t.memberExpression(t.identifier('__gantryExports'), t.identifier('default'));
        if ((t.isFunctionDeclaration(d) || t.isClassDeclaration(d)) && d.id) {
          path.replaceWith(d);
          (path as unknown as { insertAfter: (n: unknown) => void }).insertAfter(
            t.expressionStatement(t.assignmentExpression('=', target, t.identifier(d.id.name))),
          );
          return;
        }
        const expr = t.isFunctionDeclaration(d) || t.isClassDeclaration(d) ? t.toExpression(d) : d;
        path.replaceWith(t.expressionStatement(t.assignmentExpression('=', target, expr)));
      },
      ExportNamedDeclaration(path: {
        node: {
          declaration?: {
            type: string;
            id?: { name: string };
            declarations?: { id: { name: string } }[];
          } | null;
          specifiers: { exported: { name?: string; value?: string }; local: { name: string } }[];
        };
        replaceWith: (n: unknown) => void;
        remove: () => void;
        insertAfter: (n: unknown) => void;
      }) {
        // `export function App…` / `export const x` keep the declaration and register the name.
        const d = path.node.declaration;
        const register = (name: string) =>
          t.expressionStatement(
            t.assignmentExpression(
              '=',
              t.memberExpression(t.identifier('__gantryExports'), t.identifier(name)),
              t.identifier(name),
            ),
          );
        if (d) {
          const names: string[] = [];
          if (d.id?.name) names.push(d.id.name);
          for (const decl of d.declarations ?? []) if (decl.id?.name) names.push(decl.id.name);
          path.replaceWith(d);
          for (const n of names) path.insertAfter(register(n));
          return;
        }
        const statements = path.node.specifiers.map((s) =>
          t.expressionStatement(
            t.assignmentExpression(
              '=',
              t.memberExpression(
                t.identifier('__gantryExports'),
                t.identifier(s.exported.name ?? s.exported.value ?? s.local.name),
              ),
              t.identifier(s.local.name),
            ),
          ),
        );
        if (statements.length === 0) path.remove();
        else {
          path.replaceWith(statements[0]);
          for (const s of statements.slice(1)) path.insertAfter(s);
        }
      },
    },
  };
}
