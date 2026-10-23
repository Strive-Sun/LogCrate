import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import {
  SCROLLBAR_MIN_THUMB,
  scrollTopForThumbOffset,
  scrollTopForTrackPoint,
  thumbGeometry,
} from './scrollbarGeometry';

const metrics = (
  scrollTop: number,
  overrides: Partial<{ h: number; c: number; t: number }> = {},
) => ({
  scrollTop,
  scrollHeight: overrides.h ?? 10_000,
  clientHeight: overrides.c ?? 500,
  trackLength: overrides.t ?? 500,
});

describe('滚动条几何', () => {
  it('滑块长度按可见比例计算并受最小长度约束', () => {
    assert.equal(thumbGeometry(metrics(0, { h: 1_000 })).length, 250);
    assert.equal(thumbGeometry(metrics(0, { h: 1_000_000 })).length, SCROLLBAR_MIN_THUMB);
    assert.equal(
      thumbGeometry(metrics(0, { h: 600, t: 40 })).length,
      40,
      'never longer than the track',
    );
  });

  it('滑块位置按滚动比例分布在可移动范围内', () => {
    const top = thumbGeometry(metrics(0));
    const end = thumbGeometry(metrics(9_500));
    const middle = thumbGeometry(metrics(4_750));
    assert.equal(top.offset, 0);
    assert.equal(end.offset, 500 - end.length);
    assert.equal(middle.offset, (500 - middle.length) / 2);
    assert.equal(thumbGeometry(metrics(99_999)).offset, 500 - end.length, 'clamped past the end');
  });

  it('内容不足一屏或轨道不可用时不显示滑块', () => {
    assert.equal(thumbGeometry(metrics(0, { h: 500 })).visible, false);
    assert.equal(thumbGeometry(metrics(0, { h: 300 })).visible, false);
    assert.equal(thumbGeometry(metrics(0, { t: 0 })).visible, false);
    assert.equal(scrollTopForTrackPoint(metrics(0, { h: 300 }), 0, 200), 0);
  });

  it('滑块位置与 scrollTop 互逆并夹在可滚动范围内', () => {
    const m = metrics(0);
    const { length } = thumbGeometry(m);
    const travel = 500 - length;
    assert.equal(scrollTopForThumbOffset(m, length, travel / 2), 4_750);
    assert.equal(scrollTopForThumbOffset(m, length, -20), 0);
    assert.equal(scrollTopForThumbOffset(m, length, travel + 20), 9_500);
    const roundTrip = thumbGeometry({ ...m, scrollTop: scrollTopForThumbOffset(m, length, 123) });
    assert.ok(Math.abs(roundTrip.offset - 123) < 1e-9, 'offset round-trips');
  });

  it('点击轨道时滑块中心对准指针，两端跳到开头或末尾', () => {
    const m = metrics(0);
    const { length } = thumbGeometry(m);
    const target = scrollTopForTrackPoint(m, length, 300);
    assert.ok(
      Math.abs(thumbGeometry({ ...m, scrollTop: target }).offset + length / 2 - 300) < 1e-9,
    );
    assert.equal(scrollTopForTrackPoint(m, length, 0), 0);
    assert.equal(scrollTopForTrackPoint(m, length, 500), 9_500);
  });
});
