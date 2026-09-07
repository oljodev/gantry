import type { ReactNode } from 'react';

export function Window({ children }: { children: ReactNode }) {
  return (
    <div className="hw">
      <div className="hw-title">
        <span className="hw-lights" aria-hidden="true"><i /><i /><i /></span>
        <span className="hw-title-text">kinetic-api · Rate limiting on /auth</span>
        <span className="hw-chip">Claude · Auto, guard on</span>
      </div>
      <div className="hw-body">{children}</div>
    </div>
  );
}
