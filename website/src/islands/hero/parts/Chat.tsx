import type { Frame } from '../reducer';

export function Chat({ frame }: { frame: Frame }) {
  return (
    <section className="hw-chat">
      <div className="hw-user">{frame.user}</div>
      {frame.messages.map((m) => (
        <p key={m.id} className="hw-assistant">
          {m.text.slice(0, m.shown)}
          {m.streaming && <span className="hw-caret" aria-hidden="true" />}
        </p>
      ))}
      {frame.done && <p className="hw-done">Done · 1m 12s · 3 files changed</p>}
    </section>
  );
}
