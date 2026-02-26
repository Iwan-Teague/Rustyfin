'use client';

import { useEffect } from 'react';

const BURST_CLASS = 'btn-click-burst';
const BURST_DURATION_MS = 420;

function isDisabledTarget(element: HTMLElement): boolean {
  if (element.getAttribute('aria-disabled') === 'true') return true;
  if (element.matches(':disabled')) return true;
  if (element.classList.contains('disabled')) return true;
  return false;
}

function triggerBurst(element: HTMLElement) {
  if (isDisabledTarget(element)) return;
  if (element.classList.contains(BURST_CLASS)) {
    element.classList.remove(BURST_CLASS);
    // Restart animation when quickly clicking same button repeatedly.
    void element.offsetWidth;
  }
  element.classList.add(BURST_CLASS);
  window.setTimeout(() => {
    element.classList.remove(BURST_CLASS);
  }, BURST_DURATION_MS);
}

export default function PrimaryButtonEffects() {
  useEffect(() => {
    const handlePointerDown = (event: PointerEvent) => {
      const target = event.target as Element | null;
      if (!target) return;
      const button = target.closest('.btn-primary');
      if (!(button instanceof HTMLElement)) return;
      triggerBurst(button);
    };

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Enter' && event.key !== ' ') return;
      const active = document.activeElement;
      if (!(active instanceof HTMLElement)) return;
      if (!active.classList.contains('btn-primary')) return;
      triggerBurst(active);
    };

    document.addEventListener('pointerdown', handlePointerDown, true);
    document.addEventListener('keydown', handleKeyDown, true);
    return () => {
      document.removeEventListener('pointerdown', handlePointerDown, true);
      document.removeEventListener('keydown', handleKeyDown, true);
    };
  }, []);

  return null;
}
