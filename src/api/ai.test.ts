import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { mockApi } from './mock';

test('AI mock provider stores only metadata and exposes keyConfigured', async () => {
  const saved = await mockApi.saveAiProvider(
    {
      id: 'test-provider',
      name: 'Test',
      baseUrl: 'https://example.test/v1',
      model: 'test-model',
      keyConfigured: false,
      protocol: 'chatCompletions',
      endpointMode: 'base',
      allowInsecureHttp: false,
    },
    'secret-key',
  );
  assert.equal(saved.keyConfigured, true);
  const listed = await mockApi.listAiProviders();
  assert.deepEqual(listed[0], saved);
  assert.equal(JSON.stringify(listed).includes('secret-key'), false);
  await mockApi.deleteAiProvider('test-provider');
});

test('AI mock analysis rejects blank selections and returns structured content', async () => {
  await assert.rejects(() => mockApi.analyzeAiLog('missing', ' '));
  await mockApi.saveAiProvider({
    id: 'analysis',
    name: 'Analysis',
    baseUrl: 'https://example.test/v1',
    model: 'model',
    keyConfigured: true,
    protocol: 'responses',
    endpointMode: 'base',
    allowInsecureHttp: false,
  });
  const deltas: string[] = [];
  const result = await mockApi.analyzeAiLog('analysis', 'ERROR failed', (event) =>
    deltas.push(event.content),
  );
  assert.equal(result.providerId, 'analysis');
  assert.match(result.content, /ERROR/);
  assert.equal(deltas.join(''), result.content);
  assert.ok(result.timing.totalMs >= result.timing.firstContentMs!);
  await mockApi.deleteAiProvider('analysis');
});

test('Tauri AI adapter creates one request-local Channel for each invoke', () => {
  const source = readFileSync(new URL('tauri.ts', import.meta.url), 'utf8');
  assert.match(source, /new Channel<AiStreamEvent>\(\)/);
  assert.match(source, /channel\.onmessage = \(event\) => onEvent\?\.\(event\)/);
  assert.match(
    source,
    /invoke\('analyze_ai_log', \{ providerId, selectedText, onEvent: channel \}\)/,
  );
  assert.match(source, /options: \{[\s\S]*historyUpdate:[\s\S]*onEvent: channel,/);
  assert.doesNotMatch(source, /const ai(Stream)?Channel\s*=\s*new Channel/);
});

test('AI attachment contract bounds selection and returns safe display metadata', async () => {
  const summaries = await mockApi.inspectAiAttachments('ERROR failed', [
    'D:\\logs\\server.log',
    '/tmp/worker.txt',
  ]);
  assert.deepEqual(
    summaries.map(({ name, charCount }) => ({ name, charCount })),
    [
      { name: 'server.log', charCount: 0 },
      { name: 'worker.txt', charCount: 0 },
    ],
  );
  await assert.rejects(
    () =>
      mockApi.inspectAiAttachments(
        'ERROR failed',
        Array.from({ length: 6 }, (_, index) => `attachment-${index}.log`),
      ),
    /最多添加 5 个附件/,
  );
});

test('AI mock history restores sent attachment metadata with the conversation', async () => {
  await mockApi.clearAiHistory();
  await mockApi.saveAiProvider({
    id: 'history-provider',
    name: 'History',
    baseUrl: 'https://example.test/v1',
    model: 'model',
    keyConfigured: true,
    protocol: 'chatCompletions',
    endpointMode: 'base',
    allowInsecureHttp: false,
  });
  await mockApi.saveAiHistory({
    id: 'history-with-attachment',
    title: 'History',
    createdAt: '2026-08-06T00:00:00Z',
    updatedAt: '2026-08-06T00:00:00Z',
    providerId: 'history-provider',
    protocol: 'chatCompletions',
    model: 'model',
    endpointFingerprint: 'https://example.test/v1',
    selectedText: 'ERROR original',
    messages: [],
  });

  await mockApi.continueAiConversation(
    'history-provider',
    'ERROR original',
    [],
    'compare',
    ['D:\\logs\\context.log'],
    'history-with-attachment',
    '2026-08-06T00:00:01Z',
  );
  const restored = await mockApi.loadAiHistory('history-with-attachment');
  assert.equal(restored.updatedAt, '2026-08-06T00:00:01Z');
  assert.deepEqual(restored.messages[0].attachments, [{ name: 'context.log', charCount: 0 }]);
  await mockApi.deleteAiProvider('history-provider');
  await mockApi.clearAiHistory();
});
