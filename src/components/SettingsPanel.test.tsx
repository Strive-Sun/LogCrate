import assert from 'node:assert/strict';
import test, { afterEach, before } from 'node:test';
import { JSDOM } from 'jsdom';
import type { ComponentProps } from 'react';
import { I18nProvider } from '../i18n/I18nProvider';
import { SettingsPanel } from './SettingsPanel';
import { TopBar } from './TopBar';

const dom = new JSDOM('<!doctype html><html><body></body></html>', {
  url: 'http://localhost/',
  pretendToBeVisual: true,
});

let harness: typeof import('@testing-library/react');

before(async () => {
  for (const [key, value] of Object.entries({
    window: dom.window,
    document: dom.window.document,
    navigator: dom.window.navigator,
    HTMLElement: dom.window.HTMLElement,
    Node: dom.window.Node,
    localStorage: dom.window.localStorage,
    IS_REACT_ACT_ENVIRONMENT: true,
  })) {
    Object.defineProperty(globalThis, key, { configurable: true, writable: true, value });
  }
  harness = await import('@testing-library/react');
});

afterEach(() => harness.cleanup());

function renderSettings(overrides: Partial<ComponentProps<typeof SettingsPanel>> = {}) {
  const props: ComponentProps<typeof SettingsPanel> = {
    currentVersion: '1.0.0',
    autoCheck: true,
    status: 'idle',
    update: null,
    progress: null,
    error: null,
    onAutoCheckChange: () => undefined,
    onCheck: () => undefined,
    onSkip: () => undefined,
    onDownload: () => undefined,
    onClose: () => undefined,
    macOsFileAccessSupported: false,
    onOpenMacOsFileAccessSettings: () => undefined,
    searchFeature: { currentEnabled: false, nextLaunchEnabled: false },
    searchPreferenceSaving: false,
    onSearchEnabledChange: () => undefined,
    uiTemplate: 'native',
    onUiTemplateChange: () => undefined,
    ...overrides,
  };
  return harness.render(
    <I18nProvider>
      <SettingsPanel {...props} />
    </I18nProvider>,
  );
}

test('file search is disabled by default and can request enabling', () => {
  let requested: boolean | null = null;
  renderSettings({ onSearchEnabledChange: (enabled) => (requested = enabled) });
  const toggle = harness.screen.getByRole('checkbox', { name: 'Enable file search' });
  assert.equal((toggle as HTMLInputElement).checked, false);
  harness.fireEvent.click(toggle);
  assert.equal(requested, true);
});

test('pending next-launch state is visible and saving disables the toggle', () => {
  renderSettings({
    searchFeature: { currentEnabled: false, nextLaunchEnabled: true },
    searchPreferenceSaving: true,
  });
  assert.ok(harness.screen.getByText('File search will be enabled after the next restart.'));
  assert.equal(
    (harness.screen.getByRole('checkbox', { name: 'Enable file search' }) as HTMLInputElement)
      .disabled,
    true,
  );
});

test('AI provider settings follow the current version and stay collapsed until requested', () => {
  renderSettings();
  const version = harness.screen.getByText('Current version');
  const disclosure = harness.screen.getByRole('button', { name: /AI 供应商/ });
  assert.equal(
    Boolean(version.compareDocumentPosition(disclosure) & Node.DOCUMENT_POSITION_FOLLOWING),
    true,
  );
  assert.equal(disclosure.getAttribute('aria-expanded'), 'false');
  assert.equal(harness.screen.queryByRole('combobox', { name: '预设供应商' }), null);

  harness.fireEvent.click(disclosure);
  assert.equal(disclosure.getAttribute('aria-expanded'), 'true');
  assert.ok(harness.screen.getByRole('combobox', { name: '预设供应商' }));
});

test('settings shows four template previews and switches through click or arrow keys', () => {
  const requested: string[] = [];
  renderSettings({ onUiTemplateChange: (template) => requested.push(template) });

  const group = harness.screen.getByRole('radiogroup', { name: 'Interface template' });
  const radios = harness.within(group).getAllByRole('radio');
  assert.equal(radios.length, 4);
  assert.equal((radios[0] as HTMLInputElement).checked, true);
  assert.ok(harness.screen.getByText('Aurora'));
  assert.ok(harness.screen.getByText('Amber'));
  assert.ok(harness.screen.getByText('Verdant'));

  harness.fireEvent.click(radios[1]);
  assert.equal(requested.at(-1), 'aurora');
  radios[0].focus();
  harness.fireEvent.keyDown(radios[0], { key: 'ArrowLeft' });
  assert.equal(requested.at(-1), 'verdant');
  assert.equal(document.activeElement, radios[3]);
});

test('template choices remain available in a narrow settings viewport', () => {
  const originalWidth = window.innerWidth;
  Object.defineProperty(window, 'innerWidth', { configurable: true, value: 360 });
  try {
    renderSettings({ uiTemplate: 'verdant' });
    assert.equal(harness.screen.getAllByRole('radio').length, 4);
    assert.equal(
      (harness.screen.getByRole('radio', { name: /Verdant/ }) as HTMLInputElement).checked,
      true,
    );
  } finally {
    Object.defineProperty(window, 'innerWidth', { configurable: true, value: originalWidth });
  }
});

test('disabled search entry cannot be clicked and exposes its settings hint on hover', () => {
  let opened = 0;
  const props: ComponentProps<typeof TopBar> = {
    onOpenSearch: () => opened++,
    searchOpen: false,
    searchFeature: { currentEnabled: false, nextLaunchEnabled: false },
    searchPreferenceSaving: false,
    onSearchEnabledChange: () => undefined,
    uiTemplate: 'native',
    onUiTemplateChange: () => undefined,
    theme: 'light',
    onToggleTheme: () => undefined,
    count: 0,
    newItems: [],
    onOpenItem: () => undefined,
    onMarkAll: () => undefined,
    appVersion: '1.0.0',
    autoCheckUpdates: true,
    updateStatus: 'idle',
    updateInfo: null,
    updateProgress: null,
    updateError: null,
    onAutoCheckUpdatesChange: () => undefined,
    onCheckForUpdates: () => undefined,
    onSkipUpdate: () => undefined,
    onDownloadUpdate: () => undefined,
    macOsFileAccessSupported: false,
    onOpenMacOsFileAccessSettings: () => undefined,
  };
  harness.render(
    <I18nProvider>
      <TopBar {...props} />
    </I18nProvider>,
  );

  const search = harness.screen.getByRole('button', { name: /Search/ });
  assert.equal((search as HTMLButtonElement).disabled, true);
  assert.equal(search.parentElement?.title, 'Enable local file search in Settings.');
  harness.fireEvent.click(search);
  assert.equal(opened, 0);
});

test('enabled search entry opens only through normal button activation without shortcut hints', () => {
  let opened = 0;
  const props: ComponentProps<typeof TopBar> = {
    onOpenSearch: () => opened++,
    searchOpen: false,
    searchFeature: { currentEnabled: true, nextLaunchEnabled: true },
    searchPreferenceSaving: false,
    onSearchEnabledChange: () => undefined,
    uiTemplate: 'native',
    onUiTemplateChange: () => undefined,
    theme: 'light',
    onToggleTheme: () => undefined,
    count: 0,
    newItems: [],
    onOpenItem: () => undefined,
    onMarkAll: () => undefined,
    appVersion: '1.0.0',
    autoCheckUpdates: true,
    updateStatus: 'idle',
    updateInfo: null,
    updateProgress: null,
    updateError: null,
    onAutoCheckUpdatesChange: () => undefined,
    onCheckForUpdates: () => undefined,
    onSkipUpdate: () => undefined,
    onDownloadUpdate: () => undefined,
    macOsFileAccessSupported: false,
    onOpenMacOsFileAccessSettings: () => undefined,
  };
  harness.render(
    <I18nProvider>
      <TopBar {...props} />
    </I18nProvider>,
  );

  const search = harness.screen.getByRole('button', { name: /Search/ });
  assert.equal(search.title, 'Search local files');
  assert.equal(search.title.includes('Ctrl+F'), false);
  harness.fireEvent.keyDown(document, { key: 'f', ctrlKey: true });
  assert.equal(opened, 0);
  harness.fireEvent.click(search);
  assert.equal(opened, 1);
});
