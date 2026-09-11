import type { UserConfigField } from '@/bindings';

/**
 * What the form is showing, from the three things that decide it (docs/plan/03 §11 step 2): what
 * the manifest defaults to, what this instance answered last time, and what has been typed since.
 *
 * A manifest default is a *value*, not a placeholder: a number the user can see and change is one
 * they can reason about, where grey ghost text is one they have to guess at. Computing it here
 * rather than seeding it into state is what keeps the form a pure function of its inputs — the
 * effect that used to do it fired a render for every field on open.
 */
export function effectiveValues(
  fields: UserConfigField[],
  saved: Record<string, string>,
  answers: Record<string, string>,
): Record<string, string> {
  const values: Record<string, string> = {};
  for (const field of fields) {
    if (field.default != null) values[field.key] = field.default;
  }
  return { ...values, ...saved, ...answers };
}

/** Which required fields are still empty, for a button that should not be pressable yet. */
export function missingRequired(
  fields: UserConfigField[],
  values: Record<string, string>,
  hasSaved: boolean,
): string[] {
  return (
    fields
      .filter((f) => f.required)
      // A saved secret counts as filled: the form cannot show it, so an empty box means "keep".
      .filter((f) => !(f.sensitive && hasSaved))
      .filter((f) => (values[f.key] ?? '').trim() === '')
      .map((f) => f.title)
  );
}
