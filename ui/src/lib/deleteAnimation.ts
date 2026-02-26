const DELETE_ANIM_CLASS = 'tg-delete-out';
const DELETE_TARGET_CLASS = 'tg-delete-target';

function isReducedMotionPreferred(): boolean {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') {
    return false;
  }
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

export function escapeCssValue(value: string): string {
  if (typeof CSS !== 'undefined' && typeof CSS.escape === 'function') {
    return CSS.escape(value);
  }
  return value.replace(/\\/g, '\\\\').replace(/"/g, '\\"');
}

export function findDataDeleteTarget(
  attribute: string,
  value: string,
  root: ParentNode = document,
): HTMLElement | null {
  if (typeof document === 'undefined') return null;
  const selector = `[${attribute}="${escapeCssValue(value)}"]`;
  const node = root.querySelector(selector);
  return node instanceof HTMLElement ? node : null;
}

export async function playTelegramDeleteAnimation(
  element: HTMLElement | null | undefined,
  durationMs = 540,
): Promise<void> {
  if (!element) return;
  if (isReducedMotionPreferred()) return;

  element.classList.add(DELETE_TARGET_CLASS);
  if (element.classList.contains(DELETE_ANIM_CLASS)) {
    element.classList.remove(DELETE_ANIM_CLASS);
    void element.offsetWidth;
  }

  await new Promise<void>((resolve) => {
    let resolved = false;
    const done = () => {
      if (resolved) return;
      resolved = true;
      element.removeEventListener('animationend', onAnimationEnd);
      element.classList.remove(DELETE_ANIM_CLASS);
      element.classList.remove(DELETE_TARGET_CLASS);
      resolve();
    };
    const onAnimationEnd = (event: AnimationEvent) => {
      if (event.target !== element) return;
      done();
    };

    element.addEventListener('animationend', onAnimationEnd);
    element.classList.add(DELETE_ANIM_CLASS);
    window.setTimeout(done, durationMs + 180);
  });
}
