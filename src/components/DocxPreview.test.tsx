import assert from 'node:assert/strict';
import test, { afterEach, before } from 'node:test';
import { JSDOM } from 'jsdom';
import { api, type DocxPreviewBlock } from '../api';
import { I18nProvider } from '../i18n/I18nProvider';
import type * as DocxPreviewModule from './DocxPreview';

const dom = new JSDOM('<!doctype html><html><body></body></html>', {
  url: 'http://localhost/',
  pretendToBeVisual: true,
});
let harness: typeof import('@testing-library/react');
let docxModule: typeof DocxPreviewModule;
const originalReadBlocks = api.readDocxBlocks;
const originalReadImage = api.readDocxImage;
const originalCreate = URL.createObjectURL;
const originalRevoke = URL.revokeObjectURL;

before(async () => {
  class ResizeObserverStub {
    constructor(private callback: ResizeObserverCallback) {}
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
  Object.defineProperty(dom.window.HTMLElement.prototype, 'getBoundingClientRect', {
    configurable: true,
    value: () => ({
      x: 0,
      y: 0,
      top: 0,
      left: 0,
      right: 800,
      bottom: 600,
      width: 800,
      height: 600,
      toJSON: () => ({}),
    }),
  });
  Object.defineProperty(dom.window.HTMLElement.prototype, 'clientHeight', {
    configurable: true,
    get: () => 600,
  });
  Object.defineProperty(dom.window.HTMLElement.prototype, 'clientWidth', {
    configurable: true,
    get: () => 800,
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
  docxModule = await import('./DocxPreview');
});

afterEach(() => {
  api.readDocxBlocks = originalReadBlocks;
  api.readDocxImage = originalReadImage;
  URL.createObjectURL = originalCreate;
  URL.revokeObjectURL = originalRevoke;
  harness.cleanup();
});

test('DOCX text matching follows case and whole-word options', () => {
  const { docxTextMatches } = docxModule;
  assert.equal(docxTextMatches('Alpha error_1 beta', 'alpha', false, false), true);
  assert.equal(docxTextMatches('Alpha error_1 beta', 'alpha', false, true), false);
  assert.equal(docxTextMatches('an error here', 'error', true, false), true);
  assert.equal(docxTextMatches('error_1', 'error', true, false), false);
});

test('blob LRU revokes URLs when cleared', () => {
  const { DocxBlobLru } = docxModule;
  const revoked: string[] = [];
  URL.createObjectURL = () => 'blob:docx-1';
  URL.revokeObjectURL = (url) => revoked.push(url);
  const cache = new DocxBlobLru();
  assert.equal(cache.put('image-1', new Uint8Array([1, 2, 3]), 'image/png'), 'blob:docx-1');
  assert.equal(cache.get('image-1'), 'blob:docx-1');
  cache.clear();
  assert.deepEqual(revoked, ['blob:docx-1']);
});

test('blob LRU evicts the least recently used URL before its byte limit is exceeded', () => {
  const { DocxBlobLru } = docxModule;
  let sequence = 0;
  const revoked: string[] = [];
  URL.createObjectURL = () => `blob:docx-${++sequence}`;
  URL.revokeObjectURL = (url) => revoked.push(url);
  const cache = new DocxBlobLru(5);
  cache.put('first', new Uint8Array([1, 2, 3]), 'image/png');
  cache.put('second', new Uint8Array([4, 5, 6]), 'image/png');
  assert.equal(cache.get('first'), undefined);
  assert.equal(cache.get('second'), 'blob:docx-2');
  assert.deepEqual(revoked, ['blob:docx-1']);
  cache.clear();
});

test('preview pages text, lazily reads visible image, shows placeholders, and opens find', async () => {
  const { DocxPreview } = docxModule;
  const blocks: DocxPreviewBlock[] = [
    { kind: 'text', index: 0, text: 'Hello DOCX' },
    {
      kind: 'image',
      index: 1,
      imageId: 'opaque-1',
      mimeType: 'image/png',
      altText: 'Screenshot',
      status: 'supported',
    },
    {
      kind: 'image',
      index: 2,
      imageId: 'opaque-2',
      altText: 'Vector',
      status: 'unsupportedFormat',
    },
  ];
  const blockReads: Array<[number, number]> = [];
  const imageReads: string[] = [];
  api.readDocxBlocks = async (_session, start, count) => {
    blockReads.push([start, count]);
    return blocks.slice(start, start + count);
  };
  api.readDocxImage = async (_session, id) => {
    imageReads.push(id);
    return new Uint8Array([1, 2, 3]);
  };
  URL.createObjectURL = () => 'blob:visible';
  URL.revokeObjectURL = () => undefined;

  const view = harness.render(
    <I18nProvider>
      <DocxPreview
        session={{
          kind: 'docx',
          sessionId: 'docx-1',
          sourcePath: 'sample.docx',
          title: 'sample.docx',
          blockCount: 3,
          evictedSessionIds: [],
        }}
      />
    </I18nProvider>,
  );
  await harness.screen.findByText('Hello DOCX');
  await harness.screen.findByAltText('Screenshot');
  assert.ok(await harness.screen.findByText('unsupportedFormat'));
  assert.deepEqual(blockReads[0], [0, 100]);
  assert.deepEqual(imageReads, ['opaque-1']);

  harness.act(() => {
    dom.window.document.dispatchEvent(
      new dom.window.KeyboardEvent('keydown', { key: 'f', ctrlKey: true, bubbles: true }),
    );
  });
  assert.ok(await harness.screen.findByRole('dialog'));
  harness.fireEvent.input(harness.screen.getByRole('textbox', { name: 'Keyword' }), {
    target: { value: 'Hello' },
  });
  harness.fireEvent.click(harness.screen.getByRole('button', { name: 'Find' }));
  assert.ok(await harness.screen.findByText('1 / 3'));
  view.unmount();
});
