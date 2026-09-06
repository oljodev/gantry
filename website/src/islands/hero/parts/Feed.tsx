import { AnimatePresence } from 'motion/react';
import * as m from 'motion/react-m';
import type { Frame } from '../reducer';

const TOOL_LABEL: Record<string, string> = { filesystem: 'filesystem', 'code-editor': 'code-editor', shell: 'shell', github: 'github' };

export function Feed({ frame }: { frame: Frame }) {
  return (
    <aside className="hw-feed">
      <div className="hw-feed-h"><span>Activity</span><span className="hw-feed-mode">Auto · guard on</span></div>
      <ol className="hw-rows">
        <AnimatePresence initial={false}>
          {frame.rows.map((r) => (
            <m.li key={r.id} className={`hw-row is-${r.state}`} initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0 }} transition={{ duration: 0.25 }}>
              <span className="hw-row-tool">{TOOL_LABEL[r.tool]}</span>
              <span className="hw-row-action">{r.action}</span>
              <span className="hw-row-state">{r.state === 'running' ? 'running' : r.state === 'asking' ? 'allow?' : r.result}</span>
            </m.li>
          ))}
        </AnimatePresence>
      </ol>
      <AnimatePresence initial={false}>
        {frame.prompt && !frame.prompt.approved && (
          <m.div key={frame.prompt.id} className="hw-prompt" initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -4 }} transition={{ duration: 0.25 }}>
            <p className="hw-prompt-title"><span className="hw-prompt-tool">{frame.prompt.tool}</span> wants to <code>{frame.prompt.action}</code></p>
            <p className="hw-prompt-detail">{frame.prompt.detail}</p>
            <div className="hw-prompt-actions"><span className="hw-btn is-primary">Allow once</span><span className="hw-btn">Always</span><span className="hw-btn">Deny</span></div>
          </m.div>
        )}
      </AnimatePresence>
    </aside>
  );
}
