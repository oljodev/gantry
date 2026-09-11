import { describe, expect, it } from 'vitest';

import type { UserConfigField } from '@/bindings';
import { effectiveValues, missingRequired } from '@/features/connectors/config';

function field(extra: Partial<UserConfigField> & { key: string }): UserConfigField {
  return { type: 'string', title: extra.key, ...extra };
}

describe('the user_config form (03 §11 step 2)', () => {
  const fields = [
    field({ key: 'HOST', required: true }),
    field({ key: 'PORT', default: '8080' }),
    field({ key: 'TOKEN', required: true, sensitive: true }),
  ];

  it('shows a manifest default as a value, and lets both the saved answer and the typed one win', () => {
    expect(effectiveValues(fields, {}, {})).toEqual({ PORT: '8080' });
    expect(effectiveValues(fields, { PORT: '9000' }, {})).toEqual({ PORT: '9000' });
    expect(effectiveValues(fields, { PORT: '9000' }, { PORT: '' })).toEqual({ PORT: '' });
  });

  // The backend never sends a secret back, so on a configured instance an empty box means
  // "leave it alone" — treating it as missing would demand the token again on every edit.
  it('counts a saved secret as filled once the instance has been configured', () => {
    const values = { HOST: 'metabase.example' };
    expect(missingRequired(fields, values, false)).toEqual(['TOKEN']);
    expect(missingRequired(fields, values, true)).toEqual([]);
  });

  it('names every empty required field, so the button can say what it is waiting for', () => {
    expect(missingRequired(fields, {}, false)).toEqual(['HOST', 'TOKEN']);
    expect(missingRequired(fields, { HOST: '   ' }, false)).toEqual(['HOST', 'TOKEN']);
  });
});
