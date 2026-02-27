'use client';

import { RefObject, useLayoutEffect, useRef } from 'react';

const DEFAULT_EASING = 'cubic-bezier(0.18, 0.9, 0.32, 1)';
const DEFAULT_DURATION_MS = 320;

function isReducedMotionPreferred(): boolean {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') {
    return false;
  }
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

export function useListReflowAnimation(
  containerRef: RefObject<HTMLElement | null>,
  itemKeys: readonly string[],
  {
    itemSelector = '[data-list-item-id]',
    durationMs = DEFAULT_DURATION_MS,
    easing = DEFAULT_EASING,
  }: {
    itemSelector?: string;
    durationMs?: number;
    easing?: string;
  } = {},
): void {
  const previousPositionsRef = useRef<Map<string, number>>(new Map());

  useLayoutEffect(() => {
    const container = containerRef.current;
    if (!container || isReducedMotionPreferred()) return;

    const nodes = Array.from(container.querySelectorAll<HTMLElement>(itemSelector));
    const nextPositions = new Map<string, number>();

    for (const node of nodes) {
      const key = node.dataset.listItemId;
      if (!key) continue;
      nextPositions.set(key, node.getBoundingClientRect().top);
    }

    const prevPositions = previousPositionsRef.current;
    if (prevPositions.size === 0) {
      previousPositionsRef.current = nextPositions;
      return;
    }

    for (const node of nodes) {
      if (node.classList.contains('tg-delete-out')) continue;
      const key = node.dataset.listItemId;
      if (!key) continue;
      const prevTop = prevPositions.get(key);
      if (prevTop === undefined) continue;
      const nextTop = nextPositions.get(key);
      if (nextTop === undefined) continue;
      const delta = prevTop - nextTop;
      if (Math.abs(delta) < 0.5) continue;
      node.animate(
        [
          { transform: `translateY(${delta}px)` },
          { transform: 'translateY(0px)' },
        ],
        {
          duration: durationMs,
          easing,
          fill: 'both',
        },
      );
    }

    previousPositionsRef.current = nextPositions;
  }, [containerRef, itemKeys, itemSelector, durationMs, easing]);
}

