import { useEffect, useRef, useState, type RefObject } from 'react';
import { END, LOOP_PAUSE, SNAPSHOT_T } from './script';

/** Advances the session clock from the snapshot the server rendered, pauses while hidden or off-screen,
 *  and loops through a short cross-fade. With reduced motion the snapshot simply stays. */
export function usePlayback(reduced: boolean, host: RefObject<HTMLElement | null>): { t: number; fading: boolean } {
  const [t, setT] = useState(SNAPSHOT_T);
  const [fading, setFading] = useState(false);
  const visible = useRef(true);

  useEffect(() => {
    if (reduced) return;
    let raf = 0;
    let last = performance.now();
    let clock = SNAPSHOT_T;
    let fadeUntil = 0;
    const tick = (now: number) => {
      const dt = Math.min(now - last, 100);
      last = now;
      if (visible.current && !document.hidden) {
        if (fadeUntil) {
          if (now >= fadeUntil) { fadeUntil = 0; clock = 0; setFading(false); setT(0); }
        } else {
          clock += dt;
          if (clock > END + LOOP_PAUSE) { fadeUntil = now + 450; setFading(true); }
          else setT(clock);
        }
      }
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    const io = host.current && 'IntersectionObserver' in window
      ? new IntersectionObserver(([e]) => { visible.current = !!e?.isIntersecting; if (visible.current) last = performance.now(); }, { threshold: 0.05 })
      : null;
    if (host.current) io?.observe(host.current);
    const onVis = () => { if (!document.hidden) last = performance.now(); };
    document.addEventListener('visibilitychange', onVis);
    return () => { cancelAnimationFrame(raf); io?.disconnect(); document.removeEventListener('visibilitychange', onVis); };
  }, [reduced, host]);

  return { t, fading };
}
