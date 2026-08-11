export interface WindowGeometry {
  outerX: number;
  outerWidth: number;
  innerWidth: number;
  scaleFactor: number;
}

interface WindowPosition {
  x: number;
}

interface WindowSize {
  width: number;
}

interface WindowEvent<T> {
  payload: T;
}

export interface AiWorkspaceNativeWindow {
  outerPosition(): Promise<WindowPosition>;
  outerSize(): Promise<WindowSize>;
  innerSize(): Promise<WindowSize>;
  scaleFactor(): Promise<number>;
  onMoved(handler: (event: WindowEvent<WindowPosition>) => void): Promise<() => void>;
  onResized(handler: (event: WindowEvent<WindowSize>) => void): Promise<() => void>;
}

type MoveScheduler = (callback: () => void) => () => void;
export type WindowResizeEdge = 'left' | 'right' | null;

const EDGE_TOLERANCE_PX = 2;
const CURSOR_EDGE_TOLERANCE_PX = 16;
const MOVE_SETTLE_MS = 100;

const defaultMoveScheduler: MoveScheduler = (callback) => {
  const timer = window.setTimeout(callback, MOVE_SETTLE_MS);
  return () => window.clearTimeout(timer);
};

export function resizeMainWorkspaceFromWindowEdge(
  previous: WindowGeometry,
  current: WindowGeometry,
  mainWorkspaceWidth: number,
  resizeEdge: WindowResizeEdge = null,
): number {
  const previousLogicalWidth = previous.innerWidth / previous.scaleFactor;
  const currentLogicalWidth = current.innerWidth / current.scaleFactor;
  const widthDelta = currentLogicalWidth - previousLogicalWidth;
  if (Math.abs(widthDelta) < Number.EPSILON || resizeEdge === 'right') {
    return mainWorkspaceWidth;
  }
  if (resizeEdge === 'left') return Math.max(0, mainWorkspaceWidth + widthDelta);
  if (current.outerX === previous.outerX) return mainWorkspaceWidth;

  const previousRight = previous.outerX + previous.outerWidth;
  const currentRight = current.outerX + current.outerWidth;
  const rightEdgeStayedFixed = Math.abs(currentRight - previousRight) <= EDGE_TOLERANCE_PX;

  return rightEdgeStayedFixed ? Math.max(0, mainWorkspaceWidth + widthDelta) : mainWorkspaceWidth;
}

export function detectWindowResizeEdge(
  geometry: WindowGeometry,
  cursorPosition: WindowPosition,
): WindowResizeEdge {
  const leftDistance = Math.abs(cursorPosition.x - geometry.outerX);
  const rightDistance = Math.abs(cursorPosition.x - (geometry.outerX + geometry.outerWidth));
  if (leftDistance <= CURSOR_EDGE_TOLERANCE_PX && leftDistance < rightDistance) return 'left';
  if (rightDistance <= CURSOR_EDGE_TOLERANCE_PX) return 'right';
  return null;
}

export async function observeAiWorkspaceWindow(
  nativeWindow: AiWorkspaceNativeWindow,
  cursorPosition: () => Promise<WindowPosition>,
  onGeometryChanged: (
    previous: WindowGeometry,
    current: WindowGeometry,
    resizeEdge: WindowResizeEdge,
  ) => void,
  scheduleMove: MoveScheduler = defaultMoveScheduler,
): Promise<() => void> {
  const [position, outerSize, innerSize, scaleFactor] = await Promise.all([
    nativeWindow.outerPosition(),
    nativeWindow.outerSize(),
    nativeWindow.innerSize(),
    nativeWindow.scaleFactor(),
  ]);
  let geometry: WindowGeometry = {
    outerX: position.x,
    outerWidth: outerSize.width,
    innerWidth: innerSize.width,
    scaleFactor,
  };
  let disposed = false;
  let resizeGeneration = 0;
  let cancelPendingMove: (() => void) | null = null;

  const unlistenMoved = await nativeWindow.onMoved(({ payload }) => {
    cancelPendingMove?.();
    cancelPendingMove = scheduleMove(() => {
      cancelPendingMove = null;
      if (!disposed) geometry = { ...geometry, outerX: payload.x };
    });
  });
  const unlistenResized = await nativeWindow.onResized(async ({ payload }) => {
    cancelPendingMove?.();
    cancelPendingMove = null;
    const generation = ++resizeGeneration;
    const [nextPosition, nextOuterSize, nextScaleFactor, nextCursorPosition] = await Promise.all([
      nativeWindow.outerPosition(),
      nativeWindow.outerSize(),
      nativeWindow.scaleFactor(),
      cursorPosition(),
    ]);
    if (disposed || generation !== resizeGeneration) return;

    const current: WindowGeometry = {
      outerX: nextPosition.x,
      outerWidth: nextOuterSize.width,
      innerWidth: payload.width,
      scaleFactor: nextScaleFactor,
    };
    const previous = geometry;
    geometry = current;
    onGeometryChanged(previous, current, detectWindowResizeEdge(current, nextCursorPosition));
  });

  return () => {
    disposed = true;
    cancelPendingMove?.();
    unlistenMoved();
    unlistenResized();
  };
}
