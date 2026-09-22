import { useCallback, useEffect, useState } from 'react';

/**
 * Follow the bottom of a scroller while the reader is there, and stop the moment they leave.
 *
 * A streaming answer that drags the viewport along is right up until the reader wants to look at
 * something further up; from then on every new token yanks the page out from under them. So the
 * rule is the reader's position, not the turn's state: at the bottom, the newest text stays in
 * view; scrolled away, nothing moves until they come back to the bottom, which starts following
 * again on its own.
 *
 * The element arrives through `useState` rather than `useRef` on purpose. The view it lives in
 * returns a placeholder while the chat query is pending, so a `useRef` plus a mount effect
 * subscribes to nothing — the ref is still empty on the render the effect belongs to, and the
 * effect never runs again. That was the bug this hook replaced: the listener was never attached,
 * so the feed followed for ever and scrolling up did nothing.
 */

/** How close to the bottom still counts as being at the bottom, in pixels. */
const SLACK = 24;

/**
 * How far a scroll has to move before its direction is believed, in pixels.
 *
 * Reflow moves the scroller by a pixel or two on its own — a streaming line wrapping, an image
 * settling, the live turn being swapped for the stored one — and reading that as "the reader
 * scrolled up" would stop following for no reason anybody could see.
 */
const SLOP = 8;

/** Where following stands, and whether a jump of our own is still travelling. */
export interface FollowState {
  following: boolean;
  jumping: boolean;
}

/**
 * What one scroll event means for following: the rule, on its own, so it can be read and
 * tested without a scroller.
 *
 * Direction decides, not position. A notch of the wheel near the bottom leaves the reader
 * still within `SLACK` of it, so a rule written on position alone put following straight back
 * on and the next token pulled them down again — which is what made nudging the feed a little
 * impossible (2026-09-22). Coming back down to the bottom starts it again, as it always did.
 */
export function afterScroll(
  was: FollowState,
  scroll: { moved: number; bottom: boolean },
): FollowState {
  // A jump we asked for travels through positions that are not the bottom; it is over when it
  // arrives, and until then it says nothing about what the reader wants.
  if (was.jumping) {
    return scroll.bottom ? { following: true, jumping: false } : was;
  }
  if (scroll.moved < -SLOP) return { following: false, jumping: false };
  if (scroll.moved > 0 && scroll.bottom) return { following: true, jumping: false };
  return was;
}

/** Whether a scroller is at (or within a hair of) its bottom. */
export function atBottom(el: {
  scrollHeight: number;
  scrollTop: number;
  clientHeight: number;
}): boolean {
  return el.scrollHeight - el.scrollTop - el.clientHeight <= SLACK;
}

export function useFollowBottom<T extends HTMLElement>() {
  const [node, setNode] = useState<T | null>(null);
  const [following, setFollowing] = useState(true);
  // A jump of our own travels through positions that are not the bottom. Without this, the
  // scroll events it fires would read as the reader leaving and cancel the jump they asked for.
  const [jumping, setJumping] = useState(false);

  useEffect(() => {
    if (!node) return;

    // Where the scroller was at the last event, so a scroll can be read as a direction rather
    // than only as a position. Position alone was not enough: a notch of the wheel near the
    // bottom leaves the reader *still* within `SLACK` of it, so the scroll that followed the
    // gesture put following straight back on, and the next token of the answer pulled them
    // down again. Nudging the feed a little was impossible (2026-09-22).
    let lastTop = node.scrollTop;
    const onScroll = () => {
      const top = node.scrollTop;
      const moved = top - lastTop;
      lastTop = top;
      const next = afterScroll({ following, jumping }, { moved, bottom: atBottom(node) });
      setFollowing(next.following);
      setJumping(next.jumping);
    };
    // A wheel or a drag upwards is the reader saying "let me look": releasing on the gesture
    // rather than on the position it ends at means one notch is enough, and it also cancels a
    // jump that is still animating.
    const release = () => {
      setJumping(false);
      setFollowing(false);
    };
    const onWheel = (e: WheelEvent) => {
      if (e.deltaY < 0) release();
    };
    let lastY: number | null = null;
    const onTouchStart = (e: TouchEvent) => {
      lastY = e.touches[0]?.clientY ?? null;
    };
    const onTouchMove = (e: TouchEvent) => {
      const y = e.touches[0]?.clientY ?? null;
      if (lastY !== null && y !== null && y > lastY) release();
      lastY = y;
    };

    const passive = { passive: true } as const;
    node.addEventListener('scroll', onScroll, passive);
    node.addEventListener('wheel', onWheel, passive);
    node.addEventListener('touchstart', onTouchStart, passive);
    node.addEventListener('touchmove', onTouchMove, passive);
    return () => {
      node.removeEventListener('scroll', onScroll);
      node.removeEventListener('wheel', onWheel);
      node.removeEventListener('touchstart', onTouchStart);
      node.removeEventListener('touchmove', onTouchMove);
    };
  }, [node, jumping, following]);

  /** Keep the bottom in view after the content grew; a no-op once the reader has left it. */
  const stick = useCallback(() => {
    // `scrollTo`, not a write to `scrollTop`: the element is state here, and assigning to a
    // property of a state value is exactly what the compiler's immutability rule forbids.
    if (node && following) node.scrollTo({ top: node.scrollHeight, behavior: 'auto' });
  }, [node, following]);

  /** Go back to the bottom and follow again: the "New" button, and sending a message. */
  const follow = useCallback(
    (behavior: ScrollBehavior = 'smooth') => {
      if (!node) return;
      setFollowing(true);
      setJumping(behavior === 'smooth');
      node.scrollTo({ top: node.scrollHeight, behavior });
    },
    [node],
  );

  // `attach`, not `ref`: a property called `ref` reads to the compiler's lint as a ref object,
  // and every use of this hook's result would be flagged as touching a ref during render.
  // `node` comes back too, because the view measures it: how much room is left under the
  // newest turn is a question about the scroller's height.
  return { attach: setNode, node, following, stick, follow };
}
