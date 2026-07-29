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

test('log row keeps only the returned UTF-16 current match when all-match highlighting is hidden', () => {
  const view = harness.render(
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
  assert.equal(view.container.querySelectorAll('.log-find-match').length, 1);
  assert.ok(view.container.querySelector('.log-find-match-current'));
});

test('log row highlights repeated keyword fragments and distinguishes the current ERROR match', () => {
  const view = harness.render(
    <I18nProvider>
      <LogRow
        top={0}
        lineNo={4}
        line={{ lineNo: 4, content: 'ERROR token then token', truncated: false }}
        ready
        match={{ lineNo: 4, startColumn: 17, endColumn: 22 }}
        findQuery="token"
        showAllFindMatches
      />
    </I18nProvider>,
  );

  const matches = view.container.querySelectorAll('.log-find-match');
  assert.equal(matches.length, 2);
  assert.equal(matches[0].textContent, 'token');
  assert.equal(matches[0].classList.contains('log-find-match-current'), false);
  assert.equal(matches[1].textContent, 'token');
  assert.equal(matches[1].classList.contains('log-find-match-current'), true);
  assert.ok(view.container.querySelector('.lvl-ERROR'));
  assert.equal(view.container.querySelector('.log-line')?.className, 'log-line');
});

test('keyword fragments are highlighted across rendered rows and closing find keeps only current', () => {
  const rows = (showAllFindMatches: boolean) => (
    <I18nProvider>
      <LogRow
        top={0}
        lineNo={1}
        line={{ lineNo: 1, content: 'hit one hit', truncated: false }}
        ready
        match={{ lineNo: 2, startColumn: 0, endColumn: 3 }}
        findQuery="hit"
        showAllFindMatches={showAllFindMatches}
      />
      <LogRow
        top={18}
        lineNo={2}
        line={{ lineNo: 2, content: 'hit two', truncated: false }}
        ready
        match={{ lineNo: 2, startColumn: 0, endColumn: 3 }}
        findQuery="hit"
        showAllFindMatches={showAllFindMatches}
      />
    </I18nProvider>
  );
  const view = harness.render(rows(true));

  assert.equal(view.container.querySelectorAll('.log-find-match').length, 3);
  assert.equal(view.container.querySelectorAll('.log-find-match-current').length, 1);

  view.rerender(rows(false));
  assert.equal(view.container.querySelectorAll('.log-find-match').length, 1);
  assert.equal(view.container.querySelectorAll('.log-find-match-current').length, 1);
  assert.equal(view.container.querySelector('.log-find-match-current')?.textContent, 'hit');
});

test('whole-word and case-sensitive options recalculate rendered keyword fragments', () => {
  const row = (caseSensitive: boolean) => (
    <I18nProvider>
      <LogRow
        top={0}
        lineNo={3}
        line={{ lineNo: 3, content: 'Error Errors error', truncated: false }}
        ready
        findQuery="Error"
        findWholeWord
        findCaseSensitive={caseSensitive}
        showAllFindMatches
      />
    </I18nProvider>
  );
  const view = harness.render(row(true));

  assert.equal(view.container.querySelectorAll('.log-find-match').length, 1);
  view.rerender(row(false));
  assert.equal(view.container.querySelectorAll('.log-find-match').length, 2);
});
