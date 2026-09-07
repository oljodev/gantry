export function Sidebar() {
  return (
    <aside className="hw-side">
      <div className="hw-side-group">
        <p className="hw-side-h">Projects</p>
        <p className="hw-side-item is-open">kinetic-api</p>
        <p className="hw-side-sub is-active">Rate limiting on /auth</p>
        <p className="hw-side-sub">Flaky sync test</p>
        <p className="hw-side-item">driver-app</p>
      </div>
      <div className="hw-side-group">
        <p className="hw-side-h">Chats</p>
        <p className="hw-side-item">Release notes for 0.4</p>
        <p className="hw-side-item">Postgres index advice</p>
      </div>
      <div className="hw-side-group hw-side-bottom">
        <p className="hw-side-item">Connectors</p>
        <p className="hw-side-item">Settings</p>
      </div>
    </aside>
  );
}
