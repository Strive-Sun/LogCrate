import assert from 'node:assert/strict';
import test, { afterEach, before } from 'node:test';
import { JSDOM } from 'jsdom';
import type {
  FileSearchConfig,
  FileSearchPage,
  FileSearchResult,
  FileSearchStatus,
  MacOsFileAccessCapabilities,
} from '../api/types';

const dom = new JSDOM('<!doctype html><html><body></body></html>', {
  url: 'http://localhost/',
  pretendToBeVisual: true,
});

class TestResizeObserver {
  constructor(private readonly callback: ResizeObserverCallback) {}

  observe(target: Element) {
    this.callback(
      [
        {
          target,
          contentRect: target.getBoundingClientRect(),
          borderBoxSize: [],
          contentBoxSize: [],
          devicePixelContentBoxSize: [],
        } as unknown as ResizeObserverEntry,
      ],
      this as unknown as ResizeObserver,
    );
  }

  disconnect() {}
  unobserve() {}
}

before(() => {
  const globals = {
    window: dom.window,
    document: dom.window.document,
    navigator: dom.window.navigator,
    HTMLElement: dom.window.HTMLElement,
    Element: dom.window.Element,
    Node: dom.window.Node,
    MutationObserver: dom.window.MutationObserver,
    getComputedStyle: dom.window.getComputedStyle,
    localStorage: dom.window.localStorage,
    ResizeObserver: TestResizeObserver,
    requestAnimationFrame: dom.window.requestAnimationFrame.bind(dom.window),
    cancelAnimationFrame: dom.window.cancelAnimationFrame.bind(dom.window),
    IS_REACT_ACT_ENVIRONMENT: true,
  };
  for (const [key, value] of Object.entries(globals)) {
    Object.defineProperty(globalThis, key, { configurable: true, writable: true, value });
  }
  Object.defineProperty(dom.window, 'ResizeObserver', {
    configurable: true,
    value: TestResizeObserver,
  });
  Object.defineProperty(dom.window.HTMLElement.prototype, 'scrollTo', {
    configurable: true,
    value() {},
  });
  Object.defineProperty(dom.window.HTMLElement.prototype, 'getBoundingClientRect', {
    configurable: true,
    value() {
      return {
        x: 0,
        y: 0,
        top: 0,
        left: 0,
        right: 900,
        bottom: 600,
        width: 900,
        height: 600,
        toJSON() {},
      };
    },
  });
});

const status: FileSearchStatus = {
  phase: 'ready',
  scannedFiles: 2,
  skippedDirectories: 0,
  indexedFiles: 2,
  indexBytes: 128,
  roots: ['C:\\'],
  exclusions: [],
  providers: [{ root: 'C:\\', provider: 'windowsNtfs', phase: 'ready' }],
};

const result: FileSearchResult = {
  path: 'C:\\Logs\\debug.log',
  name: 'debug.log',
  parent: 'C:\\Logs',
  kind: 'log',
  size: 42,
  modifiedMs: 1_700_000_000_000,
  isLog: true,
  isArchive: false,
};

const page: FileSearchPage = {
  items: [result],
  total: 1,
  partial: false,
  elapsedMs: 2,
};

afterEach(async () => {
  const { cleanup } = await import('@testing-library/react');
  cleanup();
});

async function waitForAssertion(assertion: () => void, timeoutMs = 2_000) {
  const deadline = Date.now() + timeoutMs;
  let lastError: unknown;
  while (Date.now() < deadline) {
    try {
      assertion();
      return;
    } catch (error) {
      lastError = error;
      await new Promise((resolve) => window.setTimeout(resolve, 10));
    }
  }
  throw lastError;
}

test('搜索状态与配置请求延迟时仍立即显示可用搜索界面', async () => {
  const { render, screen } = await import('@testing-library/react');
  const React = await import('react');
  const { api } = await import('../api');
  const { FileSearchPanel } = await import('./FileSearchPanel');
  const { I18nProvider } = await import('../i18n/I18nProvider');
  const original = {
    fileSearchStatus: api.fileSearchStatus,
    fileSearchConfig: api.fileSearchConfig,
    subscribeFileSearchStatus: api.subscribeFileSearchStatus,
    macOsFileAccessCapabilities: api.macOsFileAccessCapabilities,
  };
  api.fileSearchStatus = () => new Promise<FileSearchStatus>(() => undefined);
  api.fileSearchConfig = () => new Promise<FileSearchConfig>(() => undefined);
  api.subscribeFileSearchStatus = () => () => {};
  api.macOsFileAccessCapabilities = () => new Promise<MacOsFileAccessCapabilities>(() => undefined);

  try {
    const view = render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(FileSearchPanel, {
          onClose: () => undefined,
          onOpenEntry: () => undefined,
          onMonitorAdded: async () => undefined,
          virtualizeResults: false,
        }),
      ),
    );

    assert.ok(screen.getByRole('textbox'));
    assert.equal(view.container.querySelector('.file-search-loading'), null);
    assert.ok(view.container.querySelector('.file-search-header'));
  } finally {
    Object.assign(api, original);
  }
});

async function renderPanel(overrides?: {
  inspect?: () => Promise<{
    path: string;
    name: string;
    watchPath: string;
    kind: 'file';
    isLog: boolean;
    alreadyMonitored: boolean;
  }>;
}) {
  const { act, fireEvent, render, screen } = await import('@testing-library/react');
  const React = await import('react');
  const { api } = await import('../api');
  const { FileSearchPanel } = await import('./FileSearchPanel');
  const i18n = await import('../i18n/I18nProvider');
  let searchCalls = 0;
  let monitorCalls = 0;
  const original = {
    fileSearchStatus: api.fileSearchStatus,
    fileSearchConfig: api.fileSearchConfig,
    subscribeFileSearchStatus: api.subscribeFileSearchStatus,
    macOsFileAccessCapabilities: api.macOsFileAccessCapabilities,
    searchFiles: api.searchFiles,
    inspectSearchResult: api.inspectSearchResult,
    addSearchResultParent: api.addSearchResultParent,
  };
  api.fileSearchStatus = async () => status;
  api.fileSearchConfig = async () => ({
    version: 1,
    enabled: true,
    roots: status.roots,
    exclusions: [],
  });
  api.subscribeFileSearchStatus = () => () => {};
  api.macOsFileAccessCapabilities = async () => ({
    supported: false,
    onboardingVersion: 1,
    sandboxed: false,
  });
  api.inspectSearchResult =
    overrides?.inspect ??
    (async () => ({
      path: result.path,
      name: result.name,
      watchPath: result.parent,
      kind: 'file' as const,
      isLog: true,
      alreadyMonitored: false,
    }));
  api.addSearchResultParent = async () => {
    monitorCalls += 1;
    return result.parent;
  };

  let closed = 0;
  const opened: string[] = [];
  let monitorAdded = 0;
  render(
    React.createElement(
      i18n.I18nProvider,
      null,
      React.createElement(FileSearchPanel, {
        onClose: () => {
          closed += 1;
        },
        onOpenEntry: (path: string) => opened.push(path),
        onMonitorAdded: async () => {
          monitorAdded += 1;
        },
        virtualizeResults: false,
      }),
    ),
  );
  const input = await screen.findByRole('textbox');
  const settingsButton = screen.getByRole('button', { name: 'Search index settings' });
  fireEvent.click(settingsButton);
  await screen.findByText('NTFS fast index');
  fireEvent.click(settingsButton);
  api.searchFiles = async () => {
    searchCalls += 1;
    return page;
  };
  fireEvent.change(input, { target: { value: 'debug.log' } });
  await act(async () => {
    await new Promise((resolve) => window.setTimeout(resolve, 200));
  });
  assert.ok(searchCalls > 0);
  const rowText = screen.getByText('debug.log');
  const row = rowText.closest('[role="option"]');
  assert.ok(row);

  return {
    api,
    fireEvent,
    original,
    row,
    screen,
    waitFor: waitForAssertion,
    calls: {
      get closed() {
        return closed;
      },
      get monitor() {
        return monitorCalls;
      },
      get monitorAdded() {
        return monitorAdded;
      },
      get opened() {
        return opened;
      },
      get search() {
        return searchCalls;
      },
    },
  };
}

test('搜索面板查询后双击日志并复用 LogCrate 打开链路', async () => {
  const harness = await renderPanel();
  harness.fireEvent.doubleClick(harness.row);
  await harness.waitFor(() => assert.deepEqual(harness.calls.opened, [result.path]));
  assert.equal(harness.calls.closed, 1);
  Object.assign(harness.api, harness.original);
});

test('搜索结果右键可将所在目录加入监控', async () => {
  const harness = await renderPanel();
  harness.fireEvent.contextMenu(harness.row, { clientX: 10, clientY: 10 });
  harness.fireEvent.click(harness.screen.getByText('Add containing folder to monitoring'));
  await harness.waitFor(() => assert.equal(harness.calls.monitorAdded, 1));
  assert.equal(harness.calls.monitor, 1);
  Object.assign(harness.api, harness.original);
});

test('双击已失效结果显示错误并重新查询', async () => {
  const harness = await renderPanel({
    inspect: async () => {
      throw new Error('文件已被删除或移动');
    },
  });
  const before = harness.calls.search;
  harness.fireEvent.doubleClick(harness.row);
  await harness.waitFor(() => assert.ok(harness.screen.getByText(/文件已被删除或移动/)));
  await harness.waitFor(() => assert.ok(harness.calls.search > before));
  assert.deepEqual(harness.calls.opened, []);
  Object.assign(harness.api, harness.original);
});

const fallbackStatus: FileSearchStatus = {
  ...status,
  providers: [
    {
      root: 'C:\\',
      provider: 'folderScan',
      phase: 'ready',
      stage: 'fallback',
      fallbackReason: '[missing] service not installed',
    },
  ],
};

async function renderRepairPanel(options?: { confirm?: boolean; repair?: () => Promise<void> }) {
  const { fireEvent, render, screen, waitFor } = await import('@testing-library/react');
  const React = await import('react');
  const { api } = await import('../api');
  const { FileSearchPanel } = await import('./FileSearchPanel');
  const { I18nProvider } = await import('../i18n/I18nProvider');
  const original = {
    fileSearchStatus: api.fileSearchStatus,
    fileSearchConfig: api.fileSearchConfig,
    subscribeFileSearchStatus: api.subscribeFileSearchStatus,
    macOsFileAccessCapabilities: api.macOsFileAccessCapabilities,
    repairFileSearchService: api.repairFileSearchService,
    startFileSearchIndex: api.startFileSearchIndex,
  };
  const originalConfirm = window.confirm;
  let repairCalls = 0;
  const startCalls: boolean[] = [];
  let confirmCalls = 0;
  api.fileSearchStatus = async () => fallbackStatus;
  api.fileSearchConfig = async () => ({
    version: 1,
    enabled: true,
    roots: fallbackStatus.roots,
    exclusions: [],
  });
  api.subscribeFileSearchStatus = () => () => {};
  api.macOsFileAccessCapabilities = async () => ({
    supported: false,
    onboardingVersion: 1,
    sandboxed: false,
  });
  api.repairFileSearchService = async () => {
    repairCalls += 1;
    await options?.repair?.();
  };
  api.startFileSearchIndex = async (rebuild = false) => {
    startCalls.push(rebuild);
  };
  window.confirm = () => {
    confirmCalls += 1;
    return options?.confirm ?? true;
  };

  render(
    React.createElement(
      I18nProvider,
      null,
      React.createElement(FileSearchPanel, {
        onClose: () => undefined,
        onOpenEntry: () => undefined,
        onMonitorAdded: async () => undefined,
        virtualizeResults: false,
      }),
    ),
  );
  await waitFor(() => assert.ok(screen.getByRole('textbox')));
  fireEvent.click(screen.getByRole('button', { name: 'Search index settings' }));
  const repairButton = await screen.findByRole('button', {
    name: 'Repair NTFS index service',
  });

  return {
    api,
    fireEvent,
    original,
    originalConfirm,
    repairButton,
    screen,
    waitFor,
    get confirmCalls() {
      return confirmCalls;
    },
    get repairCalls() {
      return repairCalls;
    },
    get startCalls() {
      return startCalls;
    },
  };
}

test('确认 UAC 修复成功后只重新开始一次索引并防止重复点击', async () => {
  let finishRepair!: () => void;
  const repairPending = new Promise<void>((resolve) => {
    finishRepair = resolve;
  });
  const harness = await renderRepairPanel({ repair: () => repairPending });
  harness.fireEvent.click(harness.repairButton);
  harness.fireEvent.click(harness.repairButton);
  assert.equal(harness.confirmCalls, 1);
  assert.equal(harness.repairCalls, 1);
  assert.equal(harness.repairButton.hasAttribute('disabled'), true);
  finishRepair();
  await harness.screen.findByText('The index service was repaired. Indexing has restarted.');
  assert.deepEqual(harness.startCalls, [false]);
  Object.assign(harness.api, harness.original);
  window.confirm = harness.originalConfirm;
});

test('取消修复确认时不调用后端也不重新开始索引', async () => {
  const harness = await renderRepairPanel({ confirm: false });
  harness.fireEvent.click(harness.repairButton);
  assert.equal(harness.confirmCalls, 1);
  assert.equal(harness.repairCalls, 0);
  assert.deepEqual(harness.startCalls, []);
  assert.equal(harness.repairButton.hasAttribute('disabled'), false);
  Object.assign(harness.api, harness.original);
  window.confirm = harness.originalConfirm;
});

test('八类服务修复错误均显示阶段化说明并保留兼容扫描和重试入口', async () => {
  const { cleanup } = await import('@testing-library/react');
  const cases = [
    ['missing', /still not registered/],
    ['accessDenied', /denied access/],
    ['startFailed', /could not start/],
    ['notReady', /IPC is not ready/],
    ['protocolMismatch', /protocol is incompatible/],
    ['elevationCancelled', /repair was cancelled/],
    ['repairExecutableMissing', /repair program is missing/],
    ['repairFailed', /could not be repaired/],
  ] as const;
  for (const [code, expected] of cases) {
    const harness = await renderRepairPanel({
      repair: async () => {
        throw { code, message: `diagnostic ${code}` };
      },
    });
    harness.fireEvent.click(harness.repairButton);
    await harness.screen.findByText(expected);
    assert.deepEqual(harness.startCalls, []);
    assert.ok(harness.screen.getByText('Compatible folder scan'));
    assert.equal(harness.repairButton.hasAttribute('disabled'), false);
    Object.assign(harness.api, harness.original);
    window.confirm = harness.originalConfirm;
    cleanup();
  }
});
