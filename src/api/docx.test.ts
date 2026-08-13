import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

test('DOCX uses its dedicated cancellable session path instead of log IPC', () => {
  const app = readFileSync(new URL('../App.tsx', import.meta.url), 'utf8');
  const adapter = readFileSync(new URL('tauri.ts', import.meta.url), 'utf8');
  const backend = readFileSync(new URL('../../src-tauri/src/lib.rs', import.meta.url), 'utf8');

  assert.match(app, /isDocument\s*\? await api\.openDocxSession\(entryKey, docxRequestId!\)/);
  assert.match(app, /api\.cancelOpenDocxSession\(openingRequest\)/);
  assert.doesNotMatch(app, /isDocument\s*\? await api\.openLogSession/);
  assert.match(adapter, /invoke<Omit<OpenDocxSessionResult, 'kind'>>\('open_docx_session'/);
  assert.match(adapter, /invoke\('cancel_open_docx_session'/);
  assert.match(backend, /async fn open_docx_session/);
  assert.match(backend, /async fn cancel_open_docx_session/);
});
