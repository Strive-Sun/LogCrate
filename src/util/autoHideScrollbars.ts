export const SCROLLING_CLASS = 'is-scrolling';
export const SCROLLBAR_IDLE_MS = 700;

interface ScrollTarget {
  classList: Pick<DOMTokenList, 'add' | 'remove'>;
}

interface TimeoutScheduler {
  schedule(callback: () => void, delayMs: number): unknown;
  cancel(handle: unknown): void;
}

const browserScheduler: TimeoutScheduler = {
  schedule: (callback, delayMs) => window.setTimeout(callback, delayMs),
  cancel: (handle) => window.clearTimeout(handle as number),
};

export function createAutoHideScrollbarController(
  scheduler: TimeoutScheduler = browserScheduler,
  idleMs = SCROLLBAR_IDLE_MS,
) {
  const timers = new Map<ScrollTarget, unknown>();

  return {
    scrolled(target: ScrollTarget) {
      const previous = timers.get(target);
      if (previous !== undefined) scheduler.cancel(previous);
      target.classList.add(SCROLLING_CLASS);
      const handle = scheduler.schedule(() => {
        timers.delete(target);
        target.classList.remove(SCROLLING_CLASS);
      }, idleMs);
      timers.set(target, handle);
    },

    dispose() {
      for (const [target, handle] of timers) {
        scheduler.cancel(handle);
        target.classList.remove(SCROLLING_CLASS);
      }
      timers.clear();
    },
  };
}

export function installAutoHideScrollbars(doc: Document = document): () => void {
  const controller = createAutoHideScrollbarController();
  const onScroll = (event: Event) => {
    if (event.target instanceof Element) controller.scrolled(event.target);
  };
  doc.addEventListener('scroll', onScroll, true);
  return () => {
    doc.removeEventListener('scroll', onScroll, true);
    controller.dispose();
  };
}
