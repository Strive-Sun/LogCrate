import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test, { afterEach, before } from 'node:test';
import { JSDOM } from 'jsdom';
import type { ComponentProps } from 'react';
import { I18nProvider } from '../i18n/I18nProvider';
import { LogTabs, type LogTabItem } from './LogTabs';

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
  for (const [key, value] of Object.entries({
    window: dom.window,
    document: dom.window.document,
    navigator: dom.window.navigator,
    HTMLElement: dom.window.HTMLElement,
    Node: dom.window.Node,
    ResizeObserver: ResizeObserverStub,
    localStorage: dom.window.localStorage,
    IS_REACT_ACT_ENVIRONMENT: true,
  })) {
    Object.defineProperty(globalThis, key, { configurable: true, writable: true, value });
  }
  harness = await import('@testing-library/react');
});

afterEach(() => harness.cleanup());

function tab(id: string, overrides: Partial<LogTabItem> = {}): LogTabItem {
  return { id, title: id, absolutePath: `D:\\logs\\${id}`, status: 'ready', ...overrides };
}

function renderTabs(overrides: Partial<ComponentProps<typeof LogTabs>> = {}) {
  const props: ComponentProps<typeof LogTabs> = {
    tabs: {
      'a.log': tab('a.log'),
      'b.log': tab('b.log', { sourceState: 'deleted' }),
      'c.log': tab('c.log'),
    },
    visibleIds: ['a.log', 'b.log'],
    overflowIds: ['c.log'],
    activeId: 'a.log',
    onActivate: () => undefined,
    onClose: () => undefined,
    onReload: () => undefined,
    onCapacityChange: () => undefined,
    ...overrides,
  };
  return harness.render(
    <I18nProvider>
      <LogTabs {...props} />
    </I18nProvider>,
  );
}

function openMenuOn(title: string) {
  const tabButton = harness.screen.getByRole('tab', { name: title });
  harness.fireEvent.contextMenu(tabButton, { clientX: 40, clientY: 20 });
}

test('tab context menu reloads and closes the right-clicked tab', () => {
  const reloaded: string[] = [];
  const closed: string[] = [];
  renderTabs({
    onReload: (id) => reloaded.push(id),
    onClose: (id) => closed.push(id),
  });

  openMenuOn('a.log');
  harness.fireEvent.click(harness.screen.getByRole('button', { name: 'Reload' }));
  assert.deepEqual(reloaded, ['a.log']);
  assert.equal(harness.screen.queryByRole('button', { name: 'Reload' }), null);

  openMenuOn('a.log');
  harness.fireEvent.click(harness.screen.getByRole('button', { name: 'Close' }));
  assert.deepEqual(closed, ['a.log']);
});

test('reload is disabled for a tab whose source was deleted but close still works', () => {
  const reloaded: string[] = [];
  const closed: string[] = [];
  renderTabs({
    onReload: (id) => reloaded.push(id),
    onClose: (id) => closed.push(id),
  });

  openMenuOn('b.log');
  const reload = harness.screen.getByRole('button', { name: 'Reload' }) as HTMLButtonElement;
  assert.equal(reload.disabled, true);
  harness.fireEvent.click(reload);
  assert.deepEqual(reloaded, []);
  harness.fireEvent.click(harness.screen.getByRole('button', { name: 'Close' }));
  assert.deepEqual(closed, ['b.log']);
});

test('overflow menu entries open the same tab context menu', () => {
  const reloaded: string[] = [];
  renderTabs({ onReload: (id) => reloaded.push(id) });

  harness.fireEvent.click(harness.screen.getByRole('button', { name: /More \(1\)/ }));
  harness.fireEvent.contextMenu(harness.screen.getByRole('menuitem', { name: 'c.log' }), {
    clientX: 60,
    clientY: 30,
  });
  assert.equal(harness.screen.queryByRole('menu'), null, 'overflow dropdown closes');
  harness.fireEvent.click(harness.screen.getByRole('button', { name: 'Reload' }));
  assert.deepEqual(reloaded, ['c.log']);
});

test('App routes tab reload through the explicit serialized reload path', () => {
  const app = readFileSync(new URL('../App.tsx', import.meta.url), 'utf8');
  assert.match(app, /onReload=\{reloadTab\}/);
  const reloadTab = app.slice(app.indexOf('const reloadTab = useCallback('));
  const body = reloadTab.slice(0, reloadTab.indexOf('[reloadSource]'));
  assert.match(body, /sourceState === 'deleted'\) return/);
  assert.match(body, /reloadSource\(tabSourcePath\(entryKey, tab\), undefined, 'explicit'\)/);
  assert.doesNotMatch(body, /sourceModifiedAction/);
});
