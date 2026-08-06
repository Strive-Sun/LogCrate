import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test, { afterEach, before, beforeEach } from 'node:test';
import { JSDOM } from 'jsdom';
import { useState } from 'react';
import type { ComponentProps } from 'react';
import { api, type LogFieldCondition, type LogFieldLayout, type LogSearchRequest } from '../api';
import { I18nProvider } from '../i18n/I18nProvider';
import { persistLogFieldLayout, type StoredLogFieldLayout } from '../util/logFieldLayoutStorage';
import { LogContent, mergeLogLineWindow, storedToRuntime } from './LogContent';
import { LogFieldFilterBar } from './LogFieldFilterBar';
import { LogRow } from './LogRow';

const dom = new JSDOM('<!doctype html><html><body></body></html>', {
  url: 'http://localhost/',
  pretendToBeVisual: true,
});

let harness: typeof import('@testing-library/react');
const originalSearchLog = api.searchLog;
const originalAnalyzeLogFieldLayout = api.analyzeLogFieldLayout;
const originalSetLogFieldFilter = api.setLogFieldFilter;
const originalSubscribeLogFieldProgress = api.subscribeLogFieldProgress;
const originalLogFieldStatus = api.logFieldStatus;
const originalLocateLogFieldAnchor = api.locateLogFieldAnchor;
const originalClearLogFieldFilter = api.clearLogFieldFilter;
const originalListAiProviders = api.listAiProviders;
const originalAnalyzeAiLog = api.analyzeAiLog;
const originalPrompt = dom.window.prompt;
const originalConfirm = dom.window.confirm;
const originalGetSelection = dom.window.getSelection;

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
  api.analyzeLogFieldLayout = originalAnalyzeLogFieldLayout;
  api.setLogFieldFilter = originalSetLogFieldFilter;
  api.subscribeLogFieldProgress = originalSubscribeLogFieldProgress;
  api.logFieldStatus = originalLogFieldStatus;
  api.locateLogFieldAnchor = originalLocateLogFieldAnchor;
  api.clearLogFieldFilter = originalClearLogFieldFilter;
  api.listAiProviders = originalListAiProviders;
  api.analyzeAiLog = originalAnalyzeAiLog;
  dom.window.prompt = originalPrompt;
  dom.window.confirm = originalConfirm;
  dom.window.getSelection = originalGetSelection;
  dom.window.localStorage.clear();
  harness.cleanup();
});

beforeEach(() => {
  api.analyzeLogFieldLayout = () => new Promise(() => undefined);
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

function fieldLayout(name = 'Level'): LogFieldLayout {
  return {
    fields: [
      {
        id: 'field-1',
        name: 'Time',
        fieldType: 'time',
        boundary: { start: 0, end: 19 },
        displayWidth: 19,
      },
      {
        id: 'field-2',
        name,
        fieldType: 'level',
        boundary: { start: 20, end: 25 },
        displayWidth: 5,
      },
      {
        id: 'field-3',
        name: 'Body',
        fieldType: 'text',
        boundary: { start: 26, end: null },
        displayWidth: 40,
      },
    ],
    pattern: { kind: 'bracketed', segmentCount: 2 },
    confidence: 1,
    source: 'automatic',
  };
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

test('AI analysis result opens in a closable drawer body', async () => {
  let aiWorkspaceOpened = false;
  dom.window.getSelection = () =>
    ({
      toString: () => 'ERROR synthetic failure',
    }) as Selection;
  dom.window.confirm = () => true;
  api.listAiProviders = async () => [
    {
      id: 'test-provider',
      name: 'Test provider',
      baseUrl: 'https://example.test/v1',
      model: 'test-model',
      keyConfigured: true,
      protocol: 'chatCompletions',
      endpointMode: 'base',
      allowInsecureHttp: false,
    },
  ];
  api.analyzeAiLog = async () => ({
    providerId: 'test-provider',
    model: 'test-model',
    content: 'Synthetic analysis result',
  });

  const { container } = renderLog({ onAiOpen: () => (aiWorkspaceOpened = true) });
  harness.fireEvent.contextMenu(container.querySelector('.log-view') as HTMLElement, {
    clientX: 20,
    clientY: 20,
  });
  harness.fireEvent.click(harness.screen.getByText('AI 分析'));

  const drawer = await harness.screen.findByRole('dialog', { name: 'AI 日志分析' });
  assert.ok(drawer.classList.contains('ai-result-pop'));
  assert.equal(aiWorkspaceOpened, true);
  assert.ok(drawer.querySelector('.ai-result-body'));
  assert.ok(harness.screen.getByRole('button', { name: '历史记录' }));
  assert.equal(
    harness.screen.getByText('Synthetic analysis result').textContent,
    'Synthetic analysis result',
  );

  harness.fireEvent.click(harness.screen.getByRole('button', { name: '关闭 AI 日志分析' }));
  assert.equal(harness.screen.queryByRole('dialog', { name: 'AI 日志分析' }), null);
});

test('AI workspace fills a root-level right column with an independently scrolling body', () => {
  const css = readFileSync(new URL('../styles.css', import.meta.url), 'utf8');
  const appRule = css.match(/\.app\s*\{([^}]*)\}/)?.[1] ?? '';
  const openAppRule = css.match(/\.app\.with-ai\s*\{([^}]*)\}/)?.[1] ?? '';
  const topBarRule = css.match(/\.app > \.topbar\s*\{([^}]*)\}/)?.[1] ?? '';
  const columnsRule = css.match(/\.app > \.cols\s*\{([^}]*)\}/)?.[1] ?? '';
  const searchLayerRule =
    css.match(/\.app\.with-ai > \.file-search-keep-alive\s*\{([^}]*)\}/)?.[1] ?? '';
  const hostRule = css.match(/\.ai-workspace-host\s*\{([^}]*)\}/)?.[1] ?? '';
  const drawerRule = css.match(/\.ai-result-pop\s*\{([^}]*)\}/)?.[1] ?? '';
  const headerRule = css.match(/\.ai-result-pop \.pop-head\s*\{([^}]*)\}/)?.[1] ?? '';
  const historyRule =
    css.match(/\.ai-result-pop \.pop-head > button:first-child\s*\{([^}]*)\}/)?.[1] ?? '';
  const bodyRule = css.match(/\.ai-result-body\s*\{([^}]*)\}/)?.[1] ?? '';

  assert.match(appRule, /display:\s*grid;/);
  assert.match(appRule, /grid-template-rows:\s*40px minmax\(0, 1fr\);/);
  assert.match(
    openAppRule,
    /grid-template-columns:\s*minmax\(0, 1fr\) min\(440px, calc\(100vw - 16px\)\);/,
  );
  assert.match(topBarRule, /grid-column:\s*1;/);
  assert.match(columnsRule, /grid-column:\s*1;/);
  assert.match(searchLayerRule, /right:\s*min\(440px, calc\(100vw - 16px\)\);/);
  assert.match(hostRule, /grid-row:\s*1 \/ -1;/);
  assert.match(hostRule, /height:\s*100%;/);
  assert.match(drawerRule, /position:\s*relative;/);
  assert.match(drawerRule, /width:\s*100%;/);
  assert.match(drawerRule, /height:\s*100%;/);
  assert.match(drawerRule, /overflow:\s*hidden;/);
  assert.match(headerRule, /display:\s*grid;/);
  assert.match(headerRule, /grid-template-columns:\s*1fr auto 1fr;/);
  assert.match(historyRule, /width:\s*auto;/);
  assert.match(historyRule, /font-size:\s*12px;/);
  assert.match(historyRule, /font-weight:\s*400;/);
  assert.match(bodyRule, /min-height:\s*0;/);
  assert.match(bodyRule, /overflow:\s*auto;/);
});

test('AI workspace mounts into the root host instead of covering the log panel', () => {
  const host = document.createElement('div');
  document.body.appendChild(host);
  const { container } = renderLog({ aiOpen: true, aiWorkspaceHost: host });

  const drawer = harness.screen.getByRole('dialog', { name: 'AI 日志分析' });
  assert.equal(drawer.parentElement, host);
  assert.equal(container.querySelector('.ai-result-pop'), null);
  host.remove();
});

test('AI workspace waits for its root host without flashing over the log panel', () => {
  const { container } = renderLog({ aiOpen: true, aiWorkspaceHost: null });

  assert.equal(container.querySelector('.ai-result-pop'), null);
  assert.equal(harness.screen.queryByRole('dialog', { name: 'AI 日志分析' }), null);
});

test('empty AI workspace omits the branded icon and title', () => {
  const { container } = renderLog({ aiOpen: true });

  assert.equal(container.querySelector('.ai-empty-icon'), null);
  assert.equal(harness.screen.queryByText('LogCrate AI'), null);
  assert.ok(harness.screen.getByText('选中日志后使用右键“AI 分析”，或从历史对话中继续。'));
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

test('field bar uses real fields, multi-selects values, switches result mode, and clears filters', async () => {
  const layout = {
    fields: [
      {
        id: 'field-1',
        name: 'Time',
        fieldType: 'time' as const,
        boundary: { start: 0, end: 19 },
        displayWidth: 19,
      },
      {
        id: 'field-2',
        name: 'Level',
        fieldType: 'level' as const,
        boundary: { start: 20, end: 25 },
        displayWidth: 5,
      },
      {
        id: 'field-3',
        name: 'Body',
        fieldType: 'text' as const,
        boundary: { start: 26, end: null },
        displayWidth: 40,
      },
    ],
    pattern: { kind: 'manualColumns' as const },
    confidence: 1,
    source: 'automatic' as const,
  };
  const requests: Array<{ conditions: LogFieldCondition[] }> = [];
  let generation = 0;
  let cleared = false;
  api.analyzeLogFieldLayout = async () => ({
    layout,
    sampledNonEmptyLines: 3,
    sampledBytes: 180,
    mainLayoutLines: 3,
    unparsedLines: 0,
  });
  api.setLogFieldFilter = async (_entryKey, request) => {
    requests.push(request);
    return ++generation;
  };
  api.subscribeLogFieldProgress = (_entryKey, currentGeneration, onProgress) => {
    onProgress({
      sessionId: 'session-1',
      generation: currentGeneration,
      scannedLines: 3,
      matchedLines: 2,
      unparsedLines: 1,
      totalLines: 3,
      done: true,
      failed: false,
    });
    return () => undefined;
  };
  api.logFieldStatus = async () => ({
    generation,
    layout,
    conditions: [],
    statistics: [
      {
        fieldId: 'field-2',
        candidates: [
          { value: 'INFO', count: 2 },
          { value: 'WARN', count: 1 },
        ],
        highCardinality: false,
      },
    ],
    scannedLines: 3,
    matchedLines: 2,
    unparsedLines: 1,
    totalLines: 3,
    done: true,
    failed: false,
  });
  api.locateLogFieldAnchor = async () => ({ viewIndex: 0, lineNo: 1 });
  api.clearLogFieldFilter = async () => {
    cleared = true;
  };

  const view = renderLog();
  await harness.waitFor(() => assert.ok(harness.screen.getByRole('button', { name: /^Level ▾$/ })));
  assert.equal(view.container.querySelectorAll('.log-field').length, 3);
  harness.fireEvent.click(harness.screen.getByRole('button', { name: /^Level ▾$/ }));
  await harness.waitFor(() => assert.ok(harness.screen.getByText('INFO')));
  assert.ok(harness.screen.getByLabelText('Available values'));
  assert.ok(harness.screen.getByText('0/2 selected'));
  harness.fireEvent.click(harness.screen.getByRole('checkbox', { name: /INFO/ }));
  await harness.waitFor(() => {
    assert.equal(requests.at(-1)?.conditions[0]?.kind, 'discrete');
    assert.ok(harness.screen.getByText('1/2 selected'));
  });
  harness.fireEvent.click(harness.screen.getByRole('checkbox', { name: /WARN/ }));
  await harness.waitFor(() => {
    const condition = requests.at(-1)?.conditions[0];
    assert.equal(condition?.kind, 'discrete');
    assert.deepEqual(condition?.kind === 'discrete' ? condition.values : [], ['INFO', 'WARN']);
  });
  const resultMode = harness.screen.getByRole('combobox', { name: 'Filter result mode' });
  assert.equal((resultMode as HTMLSelectElement).value, 'compact');
  harness.fireEvent.change(resultMode, { target: { value: 'highlight' } });
  assert.equal((resultMode as HTMLSelectElement).value, 'highlight');
  assert.equal(
    (harness.screen.getByRole('checkbox', { name: 'Show unparsed' }) as HTMLInputElement).disabled,
    true,
  );
  harness.fireEvent.click(harness.screen.getByRole('button', { name: 'Clear filters' }));
  await harness.waitFor(() => assert.equal(cleared, true));
});

test('field controls support text case, editing actions, keyboard resize, drag guide, and Escape', () => {
  const layout = fieldLayout('级别');
  layout.fields[0].boundary = { start: 1, end: 20 };
  layout.fields[1].boundary = { start: 23, end: 28 };
  layout.fields[2].boundary = { start: 31, end: null };
  const layoutChanges: Array<{ layout: LogFieldLayout; trigger: string }> = [];
  let conditions: LogFieldCondition[] = [];
  function ControlledBar() {
    const [currentConditions, setCurrentConditions] = useState<LogFieldCondition[]>([]);
    return (
      <LogFieldFilterBar
        layout={layout}
        conditions={currentConditions}
        statistics={[
          {
            fieldId: 'field-1',
            minTime: '2026-07-01 07:59:41.181',
            maxTime: '2026-07-31 18:42:59.999',
            candidates: [],
            highCardinality: false,
          },
          { fieldId: 'field-2', candidates: [{ value: 'INFO', count: 2 }], highCardinality: false },
        ]}
        scrollLeft={42}
        recognizing={false}
        onConditionsChange={(next) => {
          conditions = next;
          setCurrentConditions(next);
        }}
        onLayoutChange={(next, trigger) => layoutChanges.push({ layout: next, trigger })}
      />
    );
  }
  const view = harness.render(
    <I18nProvider>
      <ControlledBar />
    </I18nProvider>,
  );

  assert.equal(view.container.querySelectorAll('.log-field').length, 3);
  const fieldStyles = [...view.container.querySelectorAll<HTMLElement>('.log-field')].map(
    (field) => ({
      width: field.style.width,
      flexBasis: field.style.flexBasis,
      flexGrow: field.style.flexGrow,
      flexShrink: field.style.flexShrink,
      minWidth: field.style.minWidth,
    }),
  );
  assert.equal(new Set(fieldStyles.map((field) => field.width)).size, 1);
  assert.match(fieldStyles[0].width, /40ch/);
  assert.ok(
    fieldStyles.every(
      (field) =>
        field.flexBasis === field.width &&
        field.flexGrow === '0' &&
        field.flexShrink === '1' &&
        field.minWidth === '0',
    ),
  );
  assert.equal(view.container.querySelector('.log-field-gutter'), null);
  assert.equal(view.container.querySelector('.log-field-track')?.getAttribute('style'), null);
  assert.equal(harness.screen.getByRole('button', { name: /^级别 ▾$/ }).title, '级别');
  assert.equal(
    view.container.querySelector('.log-field-bar')?.textContent?.includes('Matches only'),
    false,
  );

  harness.fireEvent.click(harness.screen.getByRole('button', { name: /^Time ▾$/ }));
  assert.ok(harness.screen.getByLabelText('Field settings'));
  const settingsToggle = harness.screen.getByRole('button', { name: /Field settings/ });
  assert.equal(settingsToggle.getAttribute('aria-expanded'), 'false');
  assert.equal(harness.screen.queryByRole('radiogroup'), null);
  harness.fireEvent.click(settingsToggle);
  assert.equal(settingsToggle.getAttribute('aria-expanded'), 'true');
  assert.equal(harness.screen.getAllByRole('radio').length, 4);
  assert.equal(
    harness.screen.getByRole('radio', { name: 'Time' }).getAttribute('aria-checked'),
    'true',
  );
  harness.fireEvent.click(settingsToggle);
  assert.equal(settingsToggle.getAttribute('aria-expanded'), 'false');
  assert.equal(harness.screen.queryByRole('radiogroup'), null);
  harness.fireEvent.click(harness.screen.getByRole('button', { name: /^Time ▾$/ }));
  harness.fireEvent.click(harness.screen.getByRole('button', { name: /^Time ▾$/ }));
  assert.equal(
    harness.screen.getByRole('button', { name: /Field settings/ }).getAttribute('aria-expanded'),
    'false',
  );
  const startTrigger = harness.screen.getByRole('button', { name: 'Start (inclusive)' });
  harness.fireEvent.click(startTrigger);
  const startCalendar = harness.screen.getByRole('dialog', {
    name: 'Calendar for Start (inclusive)',
  });
  assert.ok(harness.screen.getByRole('button', { name: 'Previous month' }));
  harness.fireEvent.click(harness.screen.getByRole('button', { name: 'Next month' }));
  assert.match(startCalendar.textContent ?? '', /August 2026/);
  harness.fireEvent.click(harness.screen.getByRole('button', { name: 'Previous month' }));
  harness.fireEvent.click(harness.screen.getByRole('gridcell', { name: 'July 2, 2026' }));
  harness.fireEvent.change(harness.screen.getByRole('combobox', { name: 'Hour' }), {
    target: { value: '8' },
  });
  harness.fireEvent.change(harness.screen.getByRole('combobox', { name: 'Minute' }), {
    target: { value: '5' },
  });
  assert.deepEqual(conditions, [
    { kind: 'time', fieldId: 'field-1', start: '2026-07-02 08:05', end: undefined },
  ]);
  assert.ok(
    harness.screen
      .getByRole('button', { name: 'Start (inclusive)' })
      .textContent?.includes('2026-07-02 08:05'),
  );
  harness.fireEvent.click(harness.screen.getByRole('button', { name: 'Clear Start (inclusive)' }));
  assert.deepEqual(conditions, []);
  harness.fireEvent.click(harness.screen.getByRole('button', { name: 'End (inclusive)' }));
  harness.fireEvent.click(harness.screen.getByRole('gridcell', { name: 'July 1, 2026' }));
  harness.fireEvent.change(harness.screen.getByRole('combobox', { name: 'Hour' }), {
    target: { value: '8' },
  });
  harness.fireEvent.change(harness.screen.getByRole('combobox', { name: 'Minute' }), {
    target: { value: '6' },
  });
  assert.deepEqual(conditions, [
    { kind: 'time', fieldId: 'field-1', start: undefined, end: '2026-07-01 08:06' },
  ]);
  assert.ok(
    harness.screen
      .getByRole('button', { name: 'End (inclusive)' })
      .textContent?.includes('2026-07-01 08:06'),
  );
  harness.fireEvent.click(harness.screen.getByRole('button', { name: 'Clear End (inclusive)' }));
  assert.deepEqual(conditions, []);
  harness.fireEvent.click(harness.screen.getByRole('button', { name: /Field settings/ }));
  harness.fireEvent.input(harness.screen.getByRole('textbox', { name: 'Field name' }), {
    target: { value: 'Timestamp' },
  });
  harness.fireEvent.click(harness.screen.getByRole('button', { name: 'Rename' }));
  harness.fireEvent.click(harness.screen.getByRole('radio', { name: 'Text' }));
  harness.fireEvent.click(harness.screen.getByRole('button', { name: 'Split' }));
  harness.fireEvent.click(harness.screen.getByRole('button', { name: 'Merge right' }));
  const resizer = harness.screen.getByRole('button', { name: 'Resize Time' });
  harness.fireEvent.keyDown(resizer, { key: 'ArrowRight' });
  assert.deepEqual(
    layoutChanges.map((change) => change.trigger),
    ['name', 'type', 'split', 'merge', 'boundary'],
  );
  assert.equal(layoutChanges[0]?.layout.fields[0]?.name, 'Timestamp');

  harness.fireEvent.pointerDown(resizer, { clientX: 100 });
  harness.fireEvent.pointerMove(window, { clientX: 114 });
  assert.ok(view.container.querySelector('.log-field-drag-guide'));
  harness.fireEvent.pointerUp(window, { clientX: 114 });
  assert.equal(view.container.querySelector('.log-field-drag-guide'), null);

  harness.fireEvent.click(harness.screen.getByRole('button', { name: /^Body ▾$/ }));
  assert.ok(harness.screen.getByLabelText('Text filter'));
  const bodyPopover = harness.screen.getByLabelText('Text filter').closest('.log-field-popover');
  assert.ok(bodyPopover?.classList.contains('align-right'));
  assert.equal(
    harness.screen.getByRole('textbox', { name: 'Contains text' }).getAttribute('placeholder'),
    'Enter text to match',
  );
  harness.fireEvent.input(harness.screen.getByRole('textbox', { name: /Contains text/ }), {
    target: { value: 'Failure' },
  });
  const matchCase = harness.screen.getByRole('checkbox', { name: 'Match case' });
  assert.equal((matchCase as HTMLInputElement).disabled, false);
  harness.fireEvent.click(matchCase);
  assert.deepEqual(conditions, [
    {
      kind: 'text',
      fieldId: 'field-3',
      query: 'Failure',
      caseSensitive: true,
    },
  ]);
  harness.fireEvent.keyDown(matchCase, { key: 'Escape' });
  assert.equal(harness.screen.queryByRole('checkbox', { name: 'Match case' }), null);
});

test('saved layout appears immediately, validates in background, and can re-recognize mismatch', async () => {
  const saved = fieldLayout('Saved level');
  persistLogFieldLayout(
    localStorage,
    'server.log',
    {
      fields: saved.fields.map((field) => ({
        id: field.id,
        name: field.name,
        type: field.fieldType,
        start: field.boundary.start,
        end: field.boundary.end,
      })),
      fingerprint: JSON.stringify({ pattern: saved.pattern, fields: saved.fields.length }),
      encodingHint: 'UTF-8',
      source: 'automatic',
    },
    'stableAutomatic',
  );
  const recognized: LogFieldLayout = {
    ...fieldLayout('Recognized level'),
    fields: fieldLayout('Recognized level').fields.slice(0, 2),
    pattern: { kind: 'chromium' },
  };
  let confirmCalls = 0;
  dom.window.confirm = () => {
    confirmCalls += 1;
    return false;
  };
  api.analyzeLogFieldLayout = async () => ({
    layout: recognized,
    sampledNonEmptyLines: 3,
    sampledBytes: 100,
    mainLayoutLines: 3,
    unparsedLines: 0,
  });
  let generation = 0;
  api.setLogFieldFilter = async () => ++generation;
  api.subscribeLogFieldProgress = () => () => undefined;

  renderLog();
  assert.ok(harness.screen.getByRole('button', { name: /^Saved level ▾$/ }));
  await harness.waitFor(() =>
    assert.ok(harness.screen.getByRole('button', { name: /^Recognized level ▾$/ })),
  );
  assert.equal(confirmCalls, 1);
  assert.ok(harness.screen.getByText('Valid layout changes are saved automatically.'));
  assert.equal(harness.screen.queryByRole('button', { name: /^Save$/ }), null);
});

test('saved automatic chromium layouts keep parser semantics instead of fixed stale columns', () => {
  const saved: StoredLogFieldLayout = {
    fields: [
      { id: 'field-1', name: 'Time', type: 'time', start: 1, end: 18 },
      { id: 'field-2', name: 'Level', type: 'level', start: 19, end: 26 },
      { id: 'field-3', name: 'Source', type: 'discrete', start: 27, end: 48 },
      { id: 'field-4', name: 'Body', type: 'text', start: 49, end: null },
    ],
    fingerprint: JSON.stringify({ pattern: { kind: 'chromium' }, fields: 4 }),
    source: 'automatic',
  };
  assert.deepEqual(storedToRuntime(saved).pattern, { kind: 'chromium' });
});

test('low-confidence analysis falls back to an editable unsaved body field', async () => {
  api.analyzeLogFieldLayout = async () => ({
    layout: null,
    sampledNonEmptyLines: 3,
    sampledBytes: 100,
    mainLayoutLines: 0,
    unparsedLines: 3,
  });
  api.setLogFieldFilter = async () => 1;
  api.subscribeLogFieldProgress = () => () => undefined;

  const view = renderLog();
  await harness.waitFor(() => assert.ok(harness.screen.getByRole('button', { name: /^Body ▾$/ })));
  assert.ok(harness.screen.getByText(/No stable layout recognized/));
  assert.equal(localStorage.getItem('logcrate.logFieldLayouts.v1'), null);
  harness.fireEvent.click(harness.screen.getByRole('button', { name: /^Body ▾$/ }));
  harness.fireEvent.click(harness.screen.getByRole('button', { name: /Field settings/ }));
  harness.fireEvent.click(harness.screen.getByRole('button', { name: 'Split' }));
  await harness.waitFor(() =>
    assert.equal(view.container.querySelectorAll('.log-field').length, 2),
  );
  assert.ok(localStorage.getItem('logcrate.logFieldLayouts.v1'));
});

test('failed filtering returns to the unfiltered search scope with an explicit error', async () => {
  api.analyzeLogFieldLayout = async () => ({
    layout: fieldLayout(),
    sampledNonEmptyLines: 3,
    sampledBytes: 100,
    mainLayoutLines: 3,
    unparsedLines: 0,
  });
  api.setLogFieldFilter = async () => 3;
  api.subscribeLogFieldProgress = (_entryKey, generation, onProgress) => {
    onProgress({
      sessionId: 'session-1',
      generation,
      scannedLines: 2,
      matchedLines: 1,
      unparsedLines: 0,
      totalLines: 42_000,
      done: true,
      failed: true,
      error: 'scan failed',
    });
    return () => undefined;
  };
  let request: LogSearchRequest | null = null;
  api.searchLog = async (_entryKey, next) => {
    request = next;
    return {
      match: null,
      wrapped: false,
      reachedBoundary: true,
      indexedLines: 42_000,
      indexing: false,
    };
  };

  renderLog();
  await harness.waitFor(() => assert.ok(harness.screen.getByText(/scan failed/)));
  harness.fireEvent.keyDown(document, { key: 'f', ctrlKey: true });
  harness.fireEvent.input(harness.screen.getByRole('textbox', { name: 'Keyword' }), {
    target: { value: 'needle' },
  });
  harness.fireEvent.click(harness.screen.getByRole('button', { name: 'Find' }));
  await harness.waitFor(() => assert.ok(request));
  assert.equal('fieldView' in request!, false);
});

test('late window responses cannot overwrite a newer cache generation', () => {
  const current = new Map([[0, { lineNo: 1, content: 'NEW highlighted view', truncated: false }]]);
  const stale = mergeLogLineWindow(
    current,
    0,
    [{ lineNo: 99, content: 'OLD compact view', truncated: false }],
    4,
    5,
  );
  assert.equal(stale, current);
  assert.equal(stale.get(0)?.content, 'NEW highlighted view');

  const next = mergeLogLineWindow(
    current,
    1,
    [{ lineNo: 2, content: 'CURRENT view', truncated: false }],
    5,
    5,
  );
  assert.notEqual(next, current);
  assert.equal(next.get(1)?.content, 'CURRENT view');

  // A filter request clears once when it starts. The resulting render may enqueue another read
  // against the previous field generation, so activating the backend generation advances the
  // namespace a second time and rejects that intermediate response as well.
  const oldGenerationRefetch = mergeLogLineWindow(
    next,
    0,
    [{ lineNo: 1, content: 'OLD generation refetch', truncated: false }],
    5,
    6,
  );
  assert.equal(oldGenerationRefetch, next);
  assert.equal(oldGenerationRefetch.get(0)?.content, 'NEW highlighted view');
});

test('Chinese field filtering labels are available', async () => {
  localStorage.setItem('logcrate.locale', 'zh-CN');
  api.analyzeLogFieldLayout = async () => ({
    layout: fieldLayout('级别'),
    sampledNonEmptyLines: 3,
    sampledBytes: 100,
    mainLayoutLines: 3,
    unparsedLines: 0,
  });
  api.setLogFieldFilter = async () => 1;
  api.subscribeLogFieldProgress = () => () => undefined;
  renderLog();
  await harness.waitFor(() => assert.ok(harness.screen.getByRole('button', { name: /^级别 ▾$/ })));
  harness.fireEvent.click(harness.screen.getByRole('button', { name: /^Time ▾$/ }));
  harness.fireEvent.click(harness.screen.getByRole('button', { name: '开始（包含）' }));
  assert.ok(harness.screen.getByRole('dialog', { name: '「开始（包含）」日历' }));
  assert.ok(harness.screen.getByRole('button', { name: '上个月' }));
  assert.ok(harness.screen.getByRole('combobox', { name: '小时' }));
  assert.ok(harness.screen.getByRole('combobox', { name: '分钟' }));
  const resultMode = harness.screen.getByRole('combobox', { name: '筛选结果模式' });
  assert.equal((resultMode as HTMLSelectElement).value, 'compact');
  assert.deepEqual(
    Array.from((resultMode as HTMLSelectElement).options).map((option) => option.text),
    ['仅显示匹配', '高亮匹配'],
  );
  assert.ok(harness.screen.getByRole('checkbox', { name: '显示未解析' }));
  assert.ok(harness.screen.getByRole('button', { name: '清除筛选' }));
  assert.ok(harness.screen.getByText('有效的布局调整会自动保存。'));
  const filterMenu = harness.screen.getByRole('button', { name: '筛选 ▾' });
  assert.equal(filterMenu.getAttribute('aria-expanded'), 'false');
  harness.fireEvent.click(filterMenu);
  assert.equal(filterMenu.getAttribute('aria-expanded'), 'true');
  assert.ok(filterMenu.closest('.log-filter-menu')?.classList.contains('open'));
});

test('separate log tab component instances keep field conditions isolated', async () => {
  const layouts = new Map([
    ['tab-a.log', fieldLayout('Level A')],
    ['tab-b.log', fieldLayout('Level B')],
  ]);
  const generations = new Map<string, number>();
  api.analyzeLogFieldLayout = async (entryKey) => ({
    layout: layouts.get(entryKey) ?? null,
    sampledNonEmptyLines: 3,
    sampledBytes: 100,
    mainLayoutLines: 3,
    unparsedLines: 0,
  });
  api.setLogFieldFilter = async (entryKey) => {
    const generation = (generations.get(entryKey) ?? 0) + 1;
    generations.set(entryKey, generation);
    return generation;
  };
  api.subscribeLogFieldProgress = (entryKey, generation, onProgress) => {
    onProgress({
      sessionId: entryKey,
      generation,
      scannedLines: 3,
      matchedLines: 2,
      unparsedLines: 0,
      totalLines: 3,
      done: true,
      failed: false,
    });
    return () => undefined;
  };
  api.logFieldStatus = async (entryKey) => ({
    generation: generations.get(entryKey) ?? 0,
    layout: layouts.get(entryKey)!,
    conditions: [],
    statistics: [
      {
        fieldId: 'field-2',
        candidates: [{ value: 'INFO', count: 2 }],
        highCardinality: false,
      },
    ],
    scannedLines: 3,
    matchedLines: 2,
    unparsedLines: 0,
    totalLines: 3,
    done: true,
    failed: false,
  });
  const session = (sessionId: string) => ({
    sessionId,
    sourcePath: `D:\\logs\\${sessionId}`,
    entryPath: sessionId,
    size: 100,
    indexing: false,
    encoding: 'UTF-8',
    evictedSessionIds: [],
  });
  harness.render(
    <I18nProvider>
      <LogContent active activeKey="tab-a.log" session={session('tab-a.log')} />
      <LogContent active={false} activeKey="tab-b.log" session={session('tab-b.log')} />
    </I18nProvider>,
  );

  await harness.waitFor(() => {
    assert.ok(harness.screen.getByRole('button', { name: /^Level A ▾$/ }));
    assert.ok(harness.screen.getByRole('button', { name: /^Level B ▾$/ }));
  });
  harness.fireEvent.click(harness.screen.getByRole('button', { name: /^Level A ▾$/ }));
  harness.fireEvent.click(harness.screen.getByRole('checkbox', { name: /INFO/ }));
  await harness.waitFor(() =>
    assert.ok(harness.screen.getByRole('button', { name: /^Level A: 1 ▾$/ })),
  );
  assert.ok(harness.screen.getByRole('button', { name: /^Level B ▾$/ }));
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
        fieldMatched
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
  assert.equal(view.container.querySelector('.log-line')?.className, 'log-line log-field-matched');
  assert.equal(view.container.querySelectorAll('.log-field-matched mark').length, 2);
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
