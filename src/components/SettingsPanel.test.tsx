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

test('disabled search entry cannot be clicked and exposes its settings hint on hover', () => {
  let opened = 0;
  const props: ComponentProps<typeof TopBar> = {
    onOpenSearch: () => opened++,
    searchOpen: false,
    searchFeature: { currentEnabled: false, nextLaunchEnabled: false },
    searchPreferenceSaving: false,
    onSearchEnabledChange: () => undefined,
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
