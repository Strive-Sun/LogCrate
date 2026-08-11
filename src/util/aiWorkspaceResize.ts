export interface WindowGeometry {
  screenX: number;
  innerWidth: number;
}

const EDGE_TOLERANCE_PX = 2;

export function resizeMainWorkspaceFromWindowEdge(
  previous: WindowGeometry,
  current: WindowGeometry,
  mainWorkspaceWidth: number,
): number {
  const widthDelta = current.innerWidth - previous.innerWidth;
  if (widthDelta === 0 || current.screenX === previous.screenX) return mainWorkspaceWidth;

  const previousRight = previous.screenX + previous.innerWidth;
  const currentRight = current.screenX + current.innerWidth;
  const rightEdgeStayedFixed = Math.abs(currentRight - previousRight) <= EDGE_TOLERANCE_PX;

  return rightEdgeStayedFixed ? Math.max(0, mainWorkspaceWidth + widthDelta) : mainWorkspaceWidth;
}
