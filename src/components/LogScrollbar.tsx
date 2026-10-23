import { useCallback, useEffect, useRef, type PointerEvent, type RefObject } from 'react';
import {
  scrollTopForThumbOffset,
  scrollTopForTrackPoint,
  thumbGeometry,
  type ScrollMetrics,
} from '../util/scrollbarGeometry';

interface Props {
  /** 被控制的滚动容器(日志正文)。 */
  targetRef: RefObject<HTMLElement | null>;
  /** 内容高度可能变化的依据(行数、行高等),变化后重新计算滑块。 */
  contentKey: unknown;
}

/**
 * 日志正文的自绘纵向滚动条:点击轨道时滑块中心直接跳到指针位置并可继续拖动,
 * 拖动滑块时保持按下点与滑块的相对位置。几何更新直接写入滑块样式,不触发重渲染。
 */
export function LogScrollbar({ targetRef, contentKey }: Props) {
  const trackRef = useRef<HTMLDivElement>(null);
  const thumbRef = useRef<HTMLDivElement>(null);
  const frame = useRef<number | null>(null);
  // 独立的待执行标记:即使动画帧回调先于句柄返回执行,也不会让后续更新被跳过。
  const pending = useRef(false);
  // 拖动期间指针相对滑块顶部的偏移;null 表示未在拖动。
  const grabOffset = useRef<number | null>(null);

  const readMetrics = useCallback((): ScrollMetrics | null => {
    const target = targetRef.current;
    const track = trackRef.current;
    if (!target || !track) return null;
    return {
      scrollTop: target.scrollTop,
      scrollHeight: target.scrollHeight,
      clientHeight: target.clientHeight,
      // 轨道与正文可见区域等高;横向滚动条占用的高度由 render 中的底部留白让出。
      trackLength: target.clientHeight,
    };
  }, [targetRef]);

  const render = useCallback(() => {
    const metrics = readMetrics();
    const thumb = thumbRef.current;
    const target = targetRef.current;
    const track = trackRef.current;
    if (!metrics || !thumb || !target || !track) return;
    // 原生纵向滚动条宽度为 0,offsetHeight 与 clientHeight 之差即横向滚动条实际高度;
    // 没有横向滚动条时不留白,滑块才能到达正文底部。
    track.style.marginBottom = `${Math.max(0, target.offsetHeight - target.clientHeight)}px`;
    const geometry = thumbGeometry(metrics);
    thumb.hidden = !geometry.visible;
    thumb.style.height = `${geometry.length}px`;
    thumb.style.transform = `translateY(${geometry.offset}px)`;
  }, [readMetrics, targetRef]);

  const schedule = useCallback(() => {
    if (pending.current) return;
    pending.current = true;
    frame.current = window.requestAnimationFrame(() => {
      pending.current = false;
      frame.current = null;
      render();
    });
  }, [render]);

  useEffect(() => {
    const target = targetRef.current;
    const track = trackRef.current;
    if (!target || !track) return;
    target.addEventListener('scroll', schedule, { passive: true });
    const observer = new ResizeObserver(schedule);
    observer.observe(target);
    observer.observe(track);
    schedule();
    return () => {
      target.removeEventListener('scroll', schedule);
      observer.disconnect();
      if (frame.current !== null) window.cancelAnimationFrame(frame.current);
      frame.current = null;
      pending.current = false;
    };
  }, [schedule, targetRef]);

  useEffect(() => schedule(), [contentKey, schedule]);

  const pointerOffset = (event: PointerEvent) =>
    event.clientY - (trackRef.current?.getBoundingClientRect().top ?? 0);

  const scrollTo = (scrollTop: number) => {
    const target = targetRef.current;
    if (!target) return;
    target.scrollTop = scrollTop;
    render();
  };

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    const metrics = readMetrics();
    const thumb = thumbRef.current;
    if (!metrics || !thumb) return;
    const geometry = thumbGeometry(metrics);
    if (!geometry.visible) return;
    event.preventDefault();
    const offset = pointerOffset(event);
    const onThumb = event.target === thumb;
    if (onThumb) {
      grabOffset.current = offset - geometry.offset;
    } else {
      // 轨道点击:滑块中心直接对准指针,随后按“抓住中心”继续拖动。
      grabOffset.current = geometry.length / 2;
      scrollTo(scrollTopForTrackPoint(metrics, geometry.length, offset));
    }
    event.currentTarget.setPointerCapture?.(event.pointerId);
  };

  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    if (grabOffset.current === null) return;
    const metrics = readMetrics();
    if (!metrics) return;
    const { length } = thumbGeometry(metrics);
    scrollTo(scrollTopForThumbOffset(metrics, length, pointerOffset(event) - grabOffset.current));
  };

  const endDrag = (event: PointerEvent<HTMLDivElement>) => {
    grabOffset.current = null;
    event.currentTarget.releasePointerCapture?.(event.pointerId);
  };

  return (
    <div
      className="log-vscroll"
      ref={trackRef}
      aria-hidden="true"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
      onWheel={(event) => {
        const target = targetRef.current;
        if (target) target.scrollTop += event.deltaY;
      }}
    >
      <div className="log-vscroll-thumb" ref={thumbRef} hidden />
    </div>
  );
}
