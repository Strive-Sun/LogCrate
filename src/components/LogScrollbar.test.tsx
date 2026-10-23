import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test, { afterEach, before } from 'node:test';
import { JSDOM } from 'jsdom';
import { useRef } from 'react';
import { LogScrollbar } from './LogScrollbar';

const dom = new JSDOM('<!doctype html><html><body></body></html>', {
  url: 'http://localhost/',
  pretendToBeVisual: true,
});

let harness: typeof import('@testing-library/react');

before(async () => {
  class ResizeObserverStub {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  // jsdom 不提供 PointerEvent;基于 MouseEvent 补上 pointerId,其余坐标与按键字段沿用。
  class PointerEventStub extends dom.window.MouseEvent {
    pointerId: number;
    constructor(type: string, init: MouseEventInit & { pointerId?: number } = {}) {
      super(type, init);
      this.pointerId = init.pointerId ?? 1;
    }
  }
  Object.defineProperty(dom.window, 'PointerEvent', {
    configurable: true,
    value: PointerEventStub,
  });
  for (const [key, value] of Object.entries({
    window: dom.window,
    document: dom.window.document,
    navigator: dom.window.navigator,
    HTMLElement: dom.window.HTMLElement,
    Node: dom.window.Node,
    ResizeObserver: ResizeObserverStub,
    IS_REACT_ACT_ENVIRONMENT: true,
  })) {
    Object.defineProperty(globalThis, key, { configurable: true, writable: true, value });
  }
  // 同步执行动画帧,便于直接断言滑块几何。
  dom.window.requestAnimationFrame = (callback) => {
    callback(0);
    return 0;
  };
  harness = await import('@testing-library/react');
});

afterEach(() => harness.cleanup());

const TRACK_TOP = 100;
const VIEW_HEIGHT = 500;
const CONTENT_HEIGHT = 10_000;

function Fixture({
  contentHeight = CONTENT_HEIGHT,
  horizontalBar = 0,
}: {
  contentHeight?: number;
  horizontalBar?: number;
}) {
  const ref = useRef<HTMLDivElement | null>(null);
  return (
    <div>
      <div
        data-testid="view"
        ref={(element) => {
          ref.current = element;
          if (!element) return;
          Object.defineProperty(element, 'scrollHeight', {
            configurable: true,
            value: contentHeight,
          });
          Object.defineProperty(element, 'clientHeight', {
            configurable: true,
            value: VIEW_HEIGHT,
          });
          Object.defineProperty(element, 'offsetHeight', {
            configurable: true,
            value: VIEW_HEIGHT + horizontalBar,
          });
        }}
      />
      <LogScrollbar targetRef={ref} contentKey={contentHeight} />
    </div>
  );
}

function renderScrollbar(contentHeight?: number, horizontalBar?: number) {
  const view = harness.render(
    <Fixture contentHeight={contentHeight} horizontalBar={horizontalBar} />,
  );
  const track = view.container.querySelector('.log-vscroll') as HTMLElement;
  const thumb = view.container.querySelector('.log-vscroll-thumb') as HTMLElement;
  const target = harness.screen.getByTestId('view');
  track.getBoundingClientRect = () =>
    ({ top: TRACK_TOP, left: 0, right: 18, bottom: TRACK_TOP + VIEW_HEIGHT }) as DOMRect;
  harness.fireEvent.scroll(target);
  return { track, thumb, target };
}

function thumbOffset(thumb: HTMLElement) {
  return Number(/translateY\(([-\d.]+)px\)/.exec(thumb.style.transform)?.[1]);
}

test('clicking the track below the thumb jumps directly to the clicked position', () => {
  const { track, thumb, target } = renderScrollbar();
  assert.equal(thumb.hidden, false, 'thumb shown for scrollable content');
  assert.equal(thumb.style.height, '48px', 'minimum thumb length');

  harness.fireEvent.pointerDown(track, { button: 0, clientY: TRACK_TOP + 300, pointerId: 1 });
  harness.fireEvent.pointerUp(track, { button: 0, clientY: TRACK_TOP + 300, pointerId: 1 });

  // 滑块中心对准 300px:(300 - 24) / (500 - 48) × (10000 - 500)。
  assert.ok(
    Math.abs(target.scrollTop - (276 / 452) * 9_500) < 1e-6,
    `scrollTop ${target.scrollTop}`,
  );
  assert.ok(Math.abs(thumbOffset(thumb) + 24 - 300) < 1e-6, 'thumb centered on the click');
});

test('holding after a track click keeps following the pointer', () => {
  const { track, target } = renderScrollbar();
  harness.fireEvent.pointerDown(track, { button: 0, clientY: TRACK_TOP + 200, pointerId: 1 });
  harness.fireEvent.pointerMove(track, { buttons: 1, clientY: TRACK_TOP + 400, pointerId: 1 });
  assert.ok(Math.abs(target.scrollTop - (376 / 452) * 9_500) < 1e-6);
  harness.fireEvent.pointerUp(track, { clientY: TRACK_TOP + 400, pointerId: 1 });

  const settled = target.scrollTop;
  harness.fireEvent.pointerMove(track, { clientY: TRACK_TOP + 50, pointerId: 1 });
  assert.equal(target.scrollTop, settled, 'moving after release does not scroll');
});

test('dragging the thumb keeps the grab offset instead of jumping', () => {
  const { thumb, target } = renderScrollbar();
  // 滑块位于顶部 0–48px,在其中部偏下的 40px 处按下。
  harness.fireEvent.pointerDown(thumb, { button: 0, clientY: TRACK_TOP + 40, pointerId: 1 });
  assert.equal(target.scrollTop, 0, 'pressing the thumb does not jump');
  harness.fireEvent.pointerMove(thumb, { buttons: 1, clientY: TRACK_TOP + 140, pointerId: 1 });
  assert.ok(Math.abs(target.scrollTop - (100 / 452) * 9_500) < 1e-6);
  assert.ok(Math.abs(thumbOffset(thumb) - 100) < 1e-6);
});

test('clicks at the track ends jump to the start and the end', () => {
  const { track, target } = renderScrollbar();
  harness.fireEvent.pointerDown(track, {
    button: 0,
    clientY: TRACK_TOP + VIEW_HEIGHT,
    pointerId: 1,
  });
  harness.fireEvent.pointerUp(track, { pointerId: 1 });
  assert.equal(target.scrollTop, 9_500);
  harness.fireEvent.pointerDown(track, { button: 0, clientY: TRACK_TOP, pointerId: 1 });
  harness.fireEvent.pointerUp(track, { pointerId: 1 });
  assert.equal(target.scrollTop, 0);
});

test('only the primary button jumps, and content within one screen hides the thumb', () => {
  const { track, target } = renderScrollbar();
  harness.fireEvent.pointerDown(track, { button: 2, clientY: TRACK_TOP + 300, pointerId: 1 });
  assert.equal(target.scrollTop, 0, 'secondary button ignored');
  harness.cleanup();

  const short = renderScrollbar(VIEW_HEIGHT);
  assert.equal(short.thumb.hidden, true);
  harness.fireEvent.pointerDown(short.track, {
    button: 0,
    clientY: TRACK_TOP + 300,
    pointerId: 1,
  });
  assert.equal(short.target.scrollTop, 0);
});

test('wheel over the track scrolls the log body', () => {
  const { track, target } = renderScrollbar();
  harness.fireEvent.wheel(track, { deltaY: 120 });
  assert.equal(target.scrollTop, 120);
});

test('the log body hides its native vertical scrollbar and keeps the horizontal one', () => {
  const css = readFileSync(new URL('../styles.css', import.meta.url), 'utf8');
  assert.match(css, /\.log-view::-webkit-scrollbar \{\s*width: 0;\s*height: 18px;/);
  assert.match(css, /\.log-vscroll \{[^}]*flex: 0 0 18px;/);
  assert.match(css, /\.log-view-frame:has\(> \.log-view\.is-scrolling\) \{/);
  const content = readFileSync(new URL('./LogContent.tsx', import.meta.url), 'utf8');
  assert.match(content, /<LogScrollbar targetRef=\{scrollRef\}/);
});

test('hovering the track reveals the thumb immediately and leaving fades it out', () => {
  const css = readFileSync(new URL('../styles.css', import.meta.url), 'utf8');
  const hover = /\.log-vscroll:hover \{([^}]*)\}/.exec(css)?.[1] ?? '';
  assert.match(hover, /--scrollbar-thumb-color: var\(--scrollbar-thumb-hover\);/);
  assert.match(hover, /transition-duration: 0s;/);
  // 轨道自身带 450ms 过渡,悬停移出后渐隐;非悬停时继承父容器的滚动显示状态。
  assert.match(
    css,
    /\.log-vscroll \{[^}]*--scrollbar-thumb-color: inherit;[^}]*transition: --scrollbar-thumb-color 450ms ease;/,
  );
  assert.match(css, /\.log-vscroll-thumb \{[^}]*background-color: var\(--scrollbar-thumb-color\);/);
});

test('the thumb reaches the bottom of the visible log area with or without a horizontal scrollbar', () => {
  for (const horizontalBar of [0, 18]) {
    const { track, thumb, target } = renderScrollbar(CONTENT_HEIGHT, horizontalBar);
    assert.equal(track.style.marginBottom, `${horizontalBar}px`, 'gap matches the real scrollbar');
    target.scrollTop = 9_500;
    harness.fireEvent.scroll(target);
    // 滑块底边与可见区域底边重合:位置 + 长度 = 可见高度。
    assert.equal(thumbOffset(thumb) + 48, VIEW_HEIGHT);
    harness.cleanup();
  }
});
