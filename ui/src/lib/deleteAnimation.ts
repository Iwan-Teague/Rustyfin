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
  options?: { keepHiddenAtEnd?: boolean; collapse?: boolean },
): Promise<void> {
  if (!element) return;
  if (isReducedMotionPreferred()) return;

  if (options?.collapse) {
    element.classList.add(DELETE_TARGET_CLASS);
    element.style.pointerEvents = 'none';
    element.style.overflow = 'hidden';
    element.style.willChange = 'opacity, height, margin';

    const rect = element.getBoundingClientRect();
    const computed = window.getComputedStyle(element);
    const originalTransition = element.style.transition;
    const originalHeight = element.style.height;
    const originalMarginTop = element.style.marginTop;
    const originalMarginBottom = element.style.marginBottom;
    const originalOverflow = element.style.overflow;
    const originalWillChange = element.style.willChange;
    const originalOpacity = element.style.opacity;
    const originalPointerEvents = element.style.pointerEvents;

    element.style.height = `${rect.height}px`;
    element.style.marginTop = computed.marginTop;
    element.style.marginBottom = computed.marginBottom;
    void element.offsetHeight;

    element.style.transition = [
      `opacity ${durationMs}ms var(--ease-out-quart)`,
      `height ${durationMs}ms var(--ease-out-quart)`,
      `margin-top ${durationMs}ms var(--ease-out-quart)`,
      `margin-bottom ${durationMs}ms var(--ease-out-quart)`,
    ].join(', ');
    element.style.opacity = '0';
    element.style.height = '0px';
    element.style.marginTop = '0px';
    element.style.marginBottom = '0px';

    await new Promise<void>((resolve) => {
      window.setTimeout(resolve, durationMs + 120);
    });

    if (options.keepHiddenAtEnd) {
      element.style.opacity = '0';
      element.style.pointerEvents = 'none';
      return;
    }

    element.classList.remove(DELETE_TARGET_CLASS);
    element.style.transition = originalTransition;
    element.style.height = originalHeight;
    element.style.marginTop = originalMarginTop;
    element.style.marginBottom = originalMarginBottom;
    element.style.overflow = originalOverflow;
    element.style.willChange = originalWillChange;
    element.style.opacity = originalOpacity;
    element.style.pointerEvents = originalPointerEvents;
    return;
  }

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
      if (options?.keepHiddenAtEnd) {
        element.style.opacity = '0';
        element.style.pointerEvents = 'none';
      } else {
        element.classList.remove(DELETE_ANIM_CLASS);
        element.classList.remove(DELETE_TARGET_CLASS);
      }
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
