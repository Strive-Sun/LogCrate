import assert from 'node:assert/strict';
import test, { afterEach, before } from 'node:test';
import { JSDOM } from 'jsdom';
import type { ComponentProps } from 'react';
import { api, type LogSearchRequest } from '../api';
import { I18nProvider } from '../i18n/I18nProvider';
import { LogContent } from './LogContent';
import { LogRow } from './LogRow';

const dom = new JSDOM('<!doctype html><html><body></body></html>', {
  url: 'http://localhost/',
  pretendToBeVisual: true,
});

let harness: typeof import('@testing-library/react');
const originalSearchLog = api.searchLog;

before(async () => {
  class ResizeObserverStub {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  Object.defineProperty(dom.window, 'ResizeObserver', {
    configurable: true,
    value: ResizeObserverStub,
  });
  Object.defineProperty(dom.window.HTMLElement.prototype, 'scrollTo', {
    configurable: true,
    value: () => undefined,
  });
  Object.defineProperty(dom.window.HTMLElement.prototype, 'attachEvent', {
    configurable: true,
    value: () => undefined,
  });
  Object.defineProperty(dom.window.HTMLElement.prototype, 'detachEvent', {
    configurable: true,
    value: () => undefined,
  });
  for (const [key, value] of Object.entries({
    window: dom.window,
    document: dom.window.document,
    navigator: dom.window.navigator,
    HTMLElement: dom.window.HTMLElement,
    Element: dom.window.Element,
    Node: dom.window.Node,
    ResizeObserver: ResizeObserverStub,
    localStorage: dom.window.localStorage,
    IS_REACT_ACT_ENVIRONMENT: true,
  })) {
    Object.defineProperty(globalThis, key, { configurable: true, writable: true, value });
  }
  harness = await import('@testing-library/react');
});

afterEach(() => {
  api.searchLog = originalSearchLog;
  harness.cleanup();
});

function renderLog(overrides: Partial<ComponentProps<typeof LogContent>> = {}) {
  return harness.render(
    <I18nProvider>
      <LogContent
        active
        activeKey="server.log"
        session={{
          sessionId: 'session-1',
          sourcePath: 'D:\\logs\\server.log',
          entryPath: 'server.log',
          size: 1024,
          indexing: false,
          encoding: 'UTF-8',
          evictedSessionIds: [],
        }}
        {...overrides}
      />
    </I18nProvider>,
  );
}

test('Ctrl+F opens the active log find dialog with the required defaults and options', () => {
  renderLog();
  harness.fireEvent.keyDown(document, { key: 'f', ctrlKey: true });

  assert.ok(harness.screen.getByRole('dialog', { name: 'Find in log' }));
  const keyword = harness.screen.getByRole('textbox', { name: 'Keyword' });
  assert.equal(document.activeElement, keyword);
  assert.equal(
    (harness.screen.getByRole('checkbox', { name: 'Search backward' }) as HTMLInputElement).checked,
    false,
  );
  assert.equal(
    (harness.screen.getByRole('checkbox', { name: 'Whole word' }) as HTMLInputElement).checked,
    false,
  );
  assert.equal(
    (harness.screen.getByRole('checkbox', { name: 'Match case' }) as HTMLInputElement).checked,
    false,
  );
  assert.equal(
    (harness.screen.getByRole('checkbox', { name: 'Wrap search' }) as HTMLInputElement).checked,
    true,
  );
});

test('find action forwards all options and Escape closes the lightweight dialog', async () => {
  let captured: LogSearchRequest | null = null;
  api.searchLog = async (_entryKey, request) => {
    captured = request;
    return {
      match: { lineNo: 8, startColumn: 3, endColumn: 8 },
      wrapped: false,
      reachedBoundary: false,
      indexedLines: 100,
      indexing: false,
    };
  };
  renderLog();
  harness.fireEvent.keyDown(document, { key: 'f', ctrlKey: true });
  harness.fireEvent.input(harness.screen.getByRole('textbox', { name: 'Keyword' }), {
    target: { value: 'Error' },
  });
  harness.fireEvent.click(harness.screen.getByRole('checkbox', { name: 'Search backward' }));
  harness.fireEvent.click(harness.screen.getByRole('checkbox', { name: 'Whole word' }));
  harness.fireEvent.click(harness.screen.getByRole('checkbox', { name: 'Match case' }));
  harness.fireEvent.click(harness.screen.getByRole('checkbox', { name: 'Wrap search' }));
  harness.fireEvent.keyDown(harness.screen.getByRole('textbox', { name: 'Keyword' }), {
    key: 'Enter',
  });

  await harness.waitFor(() => assert.ok(captured));
  assert.deepEqual(captured, {
    query: 'Error',
    startLine: 0,
    startColumn: undefined,
    reverse: true,
    wholeWord: true,
    caseSensitive: true,
    wrap: false,
  });
  harness.fireEvent.keyDown(document, { key: 'Escape' });
  assert.equal(harness.screen.queryByRole('dialog', { name: 'Find in log' }), null);
});

test('inactive log panels do not respond to Ctrl+F', () => {
  renderLog({ active: false });
  harness.fireEvent.keyDown(document, { key: 'f', ctrlKey: true });
  assert.equal(harness.screen.queryByRole('dialog', { name: 'Find in log' }), null);
});

test('Ctrl+F does not create a find dialog without an active log session', () => {
  renderLog({ activeKey: null, session: null });
  harness.fireEvent.keyDown(document, { key: 'f', ctrlKey: true });
  assert.equal(harness.screen.queryByRole('dialog', { name: 'Find in log' }), null);
});

test('log row highlights only the returned UTF-16 match range', () => {
  harness.render(
    <I18nProvider>
      <LogRow
        top={0}
        lineNo={7}
        line={{ lineNo: 7, content: 'A😀Error tail', truncated: false }}
        ready
        match={{ lineNo: 7, startColumn: 3, endColumn: 8 }}
      />
    </I18nProvider>,
  );

  assert.equal(harness.screen.getByText('Error').tagName, 'MARK');
});
