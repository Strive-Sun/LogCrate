import assert from 'node:assert/strict';
import test from 'node:test';
import { createAutoHideScrollbarController, SCROLLING_CLASS } from './autoHideScrollbars.ts';

class FakeClassList {
  readonly values = new Set<string>();

  add(value: string) {
    this.values.add(value);
  }

  remove(value: string) {
    this.values.delete(value);
  }

  has(value: string) {
    return this.values.has(value);
  }
}

class FakeScheduler {
  private nextId = 1;
  readonly callbacks = new Map<number, () => void>();

  schedule(callback: () => void): number {
    const id = this.nextId++;
    this.callbacks.set(id, callback);
    return id;
  }

  cancel(handle: unknown) {
    this.callbacks.delete(handle as number);
  }

  run(id: number) {
    const callback = this.callbacks.get(id);
    this.callbacks.delete(id);
    callback?.();
  }
}

function target() {
  return { classList: new FakeClassList() };
}

test('重复滚动会重置当前区域的隐藏计时', () => {
  const scheduler = new FakeScheduler();
  const controller = createAutoHideScrollbarController(scheduler, 700);
  const area = target();

  controller.scrolled(area);
  const firstTimer = [...scheduler.callbacks.keys()][0];
  controller.scrolled(area);
  const secondTimer = [...scheduler.callbacks.keys()][0];

  assert.notEqual(secondTimer, firstTimer);
  assert.equal(scheduler.callbacks.has(firstTimer), false);
  assert.equal(area.classList.has(SCROLLING_CLASS), true);
  scheduler.run(secondTimer);
  assert.equal(area.classList.has(SCROLLING_CLASS), false);
});

test('多个滚动区域使用互不影响的独立计时', () => {
  const scheduler = new FakeScheduler();
  const controller = createAutoHideScrollbarController(scheduler);
  const tree = target();
  const log = target();

  controller.scrolled(tree);
  const treeTimer = [...scheduler.callbacks.keys()][0];
  controller.scrolled(log);
  const logTimer = [...scheduler.callbacks.keys()].find((id) => id !== treeTimer)!;

  scheduler.run(treeTimer);
  assert.equal(tree.classList.has(SCROLLING_CLASS), false);
  assert.equal(log.classList.has(SCROLLING_CLASS), true);
  scheduler.run(logTimer);
  assert.equal(log.classList.has(SCROLLING_CLASS), false);
});

test('销毁控制器会取消计时并清理所有显示状态', () => {
  const scheduler = new FakeScheduler();
  const controller = createAutoHideScrollbarController(scheduler);
  const tree = target();
  const log = target();

  controller.scrolled(tree);
  controller.scrolled(log);
  controller.dispose();

  assert.equal(scheduler.callbacks.size, 0);
  assert.equal(tree.classList.has(SCROLLING_CLASS), false);
  assert.equal(log.classList.has(SCROLLING_CLASS), false);
});
