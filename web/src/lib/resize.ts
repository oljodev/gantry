// Shared drag-to-resize logic for the run page's side panels — the left Tree
// rail and the right Terminal/Diff pane use the identical behavior, so it lives
// in one place. The width math is pure (unit-testable without a DOM); the hook
// wires it to pointer events and persists the last width.

import { useCallback, useEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from 'react'

export function clampWidth(width: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, Math.round(width)))
}

/** Next width for a drag. `side` is the edge the handle sits on: a left-edge
 * handle (the right pane) widens as the pointer moves left; a right-edge handle
 * (the left rail) widens as it moves right. */
export function nextWidth(
  startW: number,
  startX: number,
  clientX: number,
  side: 'left' | 'right',
  min: number,
  max: number,
): number {
  const delta = side === 'left' ? startX - clientX : clientX - startX
  return clampWidth(startW + delta, min, max)
}

export interface ResizableWidth {
  width: number
  onHandleDown: (e: ReactMouseEvent) => void
}

/** A persisted, drag-resizable width bound to a divider handle. */
export function useResizableWidth(
  storageKey: string,
  side: 'left' | 'right',
  bounds: { min: number; max: number; initial: number },
): ResizableWidth {
  const { min, max, initial } = bounds
  const [width, setWidth] = useState(() => {
    const saved = Number(localStorage.getItem(storageKey))
    return saved >= min && saved <= max ? saved : initial
  })
  const drag = useRef<{ startX: number; startW: number } | null>(null)
  const widthRef = useRef(width)
  widthRef.current = width

  useEffect(() => {
    const move = (e: globalThis.MouseEvent) => {
      if (!drag.current) return
      setWidth(nextWidth(drag.current.startW, drag.current.startX, e.clientX, side, min, max))
    }
    const up = () => {
      if (!drag.current) return
      drag.current = null
      document.body.style.userSelect = ''
      localStorage.setItem(storageKey, String(widthRef.current))
    }
    window.addEventListener('mousemove', move)
    window.addEventListener('mouseup', up)
    return () => {
      window.removeEventListener('mousemove', move)
      window.removeEventListener('mouseup', up)
    }
  }, [side, min, max, storageKey])

  const onHandleDown = useCallback((e: ReactMouseEvent) => {
    drag.current = { startX: e.clientX, startW: widthRef.current }
    document.body.style.userSelect = 'none'
  }, [])

  return { width, onHandleDown }
}
