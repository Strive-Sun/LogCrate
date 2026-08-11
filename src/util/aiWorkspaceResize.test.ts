import assert from 'node:assert/strict';
import test from 'node:test';
import {
  detectWindowResizeEdge,
  observeAiWorkspaceWindow,
  resizeMainWorkspaceFromWindowEdge,
  type AiWorkspaceNativeWindow,
  type WindowGeometry,
} from './aiWorkspaceResize';

const geometry = (
  outerX: number,
  outerWidth: number,
  innerWidth: number,
  scaleFactor = 1,
): WindowGeometry => ({ outerX, outerWidth, innerWidth, scaleFactor });

test('left-edge resize changes only the main workspace width', () => {
  assert.equal(
    resizeMainWorkspaceFromWindowEdge(geometry(100, 1456, 1440), geometry(340, 1216, 1200), 1000),
    760,
  );
  assert.equal(
    resizeMainWorkspaceFromWindowEdge(geometry(340, 1216, 1200), geometry(140, 1416, 1400), 760),
    960,
  );
});

test('right-edge resize keeps the main workspace fixed so only AI width changes', () => {
  assert.equal(
    resizeMainWorkspaceFromWindowEdge(
      geometry(100, 1456, 1440),
      geometry(100, 1216, 1200),
      1000,
      'right',
    ),
    1000,
  );
});

test('native cursor position identifies the window edge being dragged', () => {
  const current = geometry(340, 1216, 1200);
  assert.equal(detectWindowResizeEdge(current, { x: 340 }), 'left');
  assert.equal(detectWindowResizeEdge(current, { x: 1556 }), 'right');
  assert.equal(detectWindowResizeEdge(current, { x: 900 }), null);
});

test('physical width changes are converted to logical workspace pixels', () => {
  assert.equal(
    resizeMainWorkspaceFromWindowEdge(
      geometry(100, 2184, 2160, 1.5),
      geometry(400, 1884, 1860, 1.5),
      1000,
    ),
    800,
  );
});

class MockNativeWindow implements AiWorkspaceNativeWindow {
  position = { x: 100 };
  outer = { width: 1456 };
  inner = { width: 1440 };
  scale = 1;
  cursor = { x: 100 };
  movedHandler: ((event: { payload: { x: number } }) => void) | null = null;
  resizedHandler: ((event: { payload: { width: number } }) => void) | null = null;
  unlistenCount = 0;

  async outerPosition() {
    return { ...this.position };
  }

  async outerSize() {
    return { ...this.outer };
  }

  async innerSize() {
    return { ...this.inner };
  }

  async scaleFactor() {
    return this.scale;
  }

  async onMoved(handler: (event: { payload: { x: number } }) => void) {
    this.movedHandler = handler;
    return () => {
      this.unlistenCount += 1;
      this.movedHandler = null;
    };
  }

  async onResized(handler: (event: { payload: { width: number } }) => void) {
    this.resizedHandler = handler;
    return () => {
      this.unlistenCount += 1;
      this.resizedHandler = null;
    };
  }

  moveTo(x: number) {
    this.position = { x };
    this.movedHandler?.({ payload: { x } });
  }

  resizeTo(innerWidth: number, outerWidth: number) {
    this.inner = { width: innerWidth };
    this.outer = { width: outerWidth };
    this.resizedHandler?.({ payload: { width: innerWidth } });
  }
}

const flushNativeQueries = async () => {
  await Promise.resolve();
  await Promise.resolve();
};

test('native cursor keeps AI width fixed even if moved settles before left resize', async () => {
  const nativeWindow = new MockNativeWindow();
  let pendingMove: (() => void) | null = null;
  let mainWidth = 1000;
  const stop = await observeAiWorkspaceWindow(
    nativeWindow,
    async () => ({ ...nativeWindow.cursor }),
    (previous, current, resizeEdge) => {
      mainWidth = resizeMainWorkspaceFromWindowEdge(previous, current, mainWidth, resizeEdge);
    },
    (callback) => {
      pendingMove = () => {
        pendingMove = null;
        callback();
      };
      return () => {
        pendingMove = null;
      };
    },
  );

  nativeWindow.cursor = { x: 340 };
  nativeWindow.moveTo(340);
  assert.ok(pendingMove);
  (pendingMove as () => void)();
  nativeWindow.resizeTo(1200, 1216);
  await flushNativeQueries();
  assert.equal(mainWidth, 760, 'all lost width must come from the main workspace');

  stop();
  assert.equal(nativeWindow.unlistenCount, 2);
});

test('a settled whole-window move does not turn a later right-edge resize into a left resize', async () => {
  const nativeWindow = new MockNativeWindow();
  let pendingMove: (() => void) | null = null;
  let mainWidth = 1000;
  const stop = await observeAiWorkspaceWindow(
    nativeWindow,
    async () => ({ ...nativeWindow.cursor }),
    (previous, current, resizeEdge) => {
      mainWidth = resizeMainWorkspaceFromWindowEdge(previous, current, mainWidth, resizeEdge);
    },
    (callback) => {
      pendingMove = () => {
        pendingMove = null;
        callback();
      };
      return () => {
        pendingMove = null;
      };
    },
  );

  nativeWindow.moveTo(300);
  assert.ok(pendingMove);
  (pendingMove as () => void)();
  nativeWindow.cursor = { x: 1516 };
  nativeWindow.resizeTo(1200, 1216);
  await flushNativeQueries();
  assert.equal(
    mainWidth,
    1000,
    'right-edge dragging must continue to resize only the AI workspace',
  );

  stop();
});
