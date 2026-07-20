import assert from 'node:assert/strict';
import test from 'node:test';
import {
  activateTab,
  closeTab,
  emptyTabLayout,
  markEvictedSessions,
  openTab,
  resizeTabs,
  tabIds,
} from './logTabs';

test('opening unique tabs fills visible slots and then displaces the rightmost tab', () => {
  let layout = emptyTabLayout(3);
  layout = openTab(layout, 'a');
  layout = openTab(layout, 'b');
  layout = openTab(layout, 'c');
  layout = openTab(layout, 'd');
  assert.deepEqual(layout, {
    visible: ['a', 'b', 'd'],
    overflow: ['c'],
    active: 'd',
    capacity: 3,
  });
});

test('activating an overflow tab swaps it with the rightmost visible tab in place', () => {
  const layout = activateTab(
    { visible: ['a', 'b', 'c'], overflow: ['d', 'e'], active: 'b', capacity: 3 },
    'e',
  );
  assert.deepEqual(layout.visible, ['a', 'b', 'e']);
  assert.deepEqual(layout.overflow, ['d', 'c']);
  assert.equal(layout.active, 'e');
});

test('opening an existing tab only activates it', () => {
  const initial = { visible: ['a', 'b'], overflow: [], active: 'a', capacity: 2 };
  const layout = openTab(initial, 'b');
  assert.deepEqual(tabIds(layout), ['a', 'b']);
  assert.equal(layout.active, 'b');
});

test('resize keeps the active tab visible and restores overflow order when growing', () => {
  const compact = resizeTabs(
    { visible: ['a', 'b', 'c', 'd'], overflow: ['e'], active: 'd', capacity: 4 },
    2,
  );
  assert.deepEqual(compact.visible, ['a', 'd']);
  assert.deepEqual(compact.overflow, ['e', 'c', 'b']);
  assert.ok(compact.visible.includes(compact.active!));

  const expanded = resizeTabs(compact, 4);
  assert.deepEqual(expanded.visible, ['a', 'd', 'e', 'c']);
  assert.deepEqual(expanded.overflow, ['b']);
});

test('capacity one swaps through overflow without losing tabs', () => {
  let layout = emptyTabLayout(1);
  layout = openTab(layout, 'a');
  layout = openTab(layout, 'b');
  layout = activateTab(layout, 'a');
  assert.deepEqual(layout.visible, ['a']);
  assert.deepEqual(layout.overflow, ['b']);
  assert.equal(layout.active, 'a');
});

test('closing tabs selects a neighbor and fills visible vacancies from overflow', () => {
  const activeClosed = closeTab(
    { visible: ['a', 'b', 'c'], overflow: ['d', 'e'], active: 'b', capacity: 3 },
    'b',
  );
  assert.deepEqual(activeClosed.visible, ['a', 'c', 'd']);
  assert.deepEqual(activeClosed.overflow, ['e']);
  assert.equal(activeClosed.active, 'c');

  const hiddenClosed = closeTab(activeClosed, 'e');
  assert.deepEqual(hiddenClosed.overflow, []);
  assert.equal(hiddenClosed.active, 'c');
});

test('LRU eviction makes only matching session tabs dormant', () => {
  const tabs = {
    a: { session: { sessionId: 's1' }, status: 'ready' },
    b: { session: { sessionId: 's2' }, status: 'ready' },
    c: { session: null, status: 'opening' },
  };
  const next = markEvictedSessions(tabs, ['s1']);
  assert.deepEqual(next.a, { session: null, status: 'dormant' });
  assert.equal(next.b, tabs.b);
  assert.equal(next.c, tabs.c);
});
