export const SCROLLBAR_MIN_THUMB = 48;

export interface ScrollMetrics {
  scrollTop: number;
  scrollHeight: number;
  clientHeight: number;
  /** 自绘轨道的可用长度(px)。 */
  trackLength: number;
}

export interface ThumbGeometry {
  /** 内容不足一屏或轨道过短时不显示滑块。 */
  visible: boolean;
  offset: number;
  length: number;
}

function maxScroll(metrics: ScrollMetrics): number {
  return Math.max(0, metrics.scrollHeight - metrics.clientHeight);
}

/** 按可见比例计算滑块长度(不小于最小长度、不超过轨道),再按滚动比例计算滑块位置。 */
export function thumbGeometry(
  metrics: ScrollMetrics,
  minThumb = SCROLLBAR_MIN_THUMB,
): ThumbGeometry {
  const range = maxScroll(metrics);
  const track = Math.max(0, metrics.trackLength);
  if (range <= 0 || track <= 0 || metrics.scrollHeight <= 0) {
    return { visible: false, offset: 0, length: 0 };
  }
  const proportional = (track * metrics.clientHeight) / metrics.scrollHeight;
  const length = Math.min(track, Math.max(minThumb, proportional));
  const travel = track - length;
  const ratio = Math.min(1, Math.max(0, metrics.scrollTop / range));
  return { visible: true, offset: ratio * travel, length };
}

/** 把滑块顶部位置换算为 scrollTop,并夹在可滚动范围内。 */
export function scrollTopForThumbOffset(
  metrics: ScrollMetrics,
  thumbLength: number,
  thumbOffset: number,
): number {
  const range = maxScroll(metrics);
  const travel = Math.max(0, metrics.trackLength - thumbLength);
  if (range <= 0 || travel <= 0) return 0;
  const ratio = Math.min(1, Math.max(0, thumbOffset / travel));
  return ratio * range;
}

/** 点击轨道时让滑块中心对准指针位置。 */
export function scrollTopForTrackPoint(
  metrics: ScrollMetrics,
  thumbLength: number,
  pointerOffset: number,
): number {
  return scrollTopForThumbOffset(metrics, thumbLength, pointerOffset - thumbLength / 2);
}
