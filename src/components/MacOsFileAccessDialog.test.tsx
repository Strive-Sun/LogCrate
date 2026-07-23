import assert from 'node:assert/strict';
import test, { afterEach, before } from 'node:test';
import { JSDOM } from 'jsdom';
import { I18nProvider } from '../i18n/I18nProvider';
import { MacOsFileAccessDialog } from './MacOsFileAccessDialog';

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

test('macOS access dialog opens settings only after explicit user action', () => {
  let opened = 0;
  let deferred = 0;
  harness.render(
    <I18nProvider>
      <MacOsFileAccessDialog onLater={() => deferred++} onOpenSettings={() => opened++} />
    </I18nProvider>,
  );

  assert.equal(opened, 0);
  harness.fireEvent.click(harness.screen.getByRole('button', { name: 'Open System Settings' }));
  assert.equal(opened, 1);
  assert.equal(deferred, 0);
});

test('macOS access dialog can be deferred with Escape', () => {
  let deferred = 0;
  harness.render(
    <I18nProvider>
      <MacOsFileAccessDialog onLater={() => deferred++} onOpenSettings={() => undefined} />
    </I18nProvider>,
  );
  harness.fireEvent.keyDown(document, { key: 'Escape' });
  assert.equal(deferred, 1);
});
