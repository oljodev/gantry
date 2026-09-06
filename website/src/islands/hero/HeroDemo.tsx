import { useRef } from 'react';
import { LazyMotion, domAnimation, useReducedMotion } from 'motion/react';
import { frameAt } from './reducer';
import { usePlayback } from './usePlayback';
import { Window } from './parts/Window';
import { Sidebar } from './parts/Sidebar';
import { Chat } from './parts/Chat';
import { Feed } from './parts/Feed';
import './hero.css';

/** The hero's product mock: a Gantry window playing a short session. The server renders a mid-session frame;
 *  the client resumes from it. Decorative for assistive tech (the section carries a text description). */
export default function HeroDemo() {
  const reduced = useReducedMotion() ?? false;
  const host = useRef<HTMLDivElement>(null);
  const { t, fading } = usePlayback(reduced, host);
  const frame = frameAt(t);
  return (
    <LazyMotion features={domAnimation} strict>
      <div ref={host} className={`hero-window${fading ? ' is-fading' : ''}`} aria-hidden="true">
        <Window>
          <Sidebar />
          <Chat frame={frame} />
          <Feed frame={frame} />
        </Window>
      </div>
    </LazyMotion>
  );
}
