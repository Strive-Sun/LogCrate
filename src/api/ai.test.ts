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
  assert.match(source, /options: \{[\s\S]*attachmentPaths,[\s\S]*logSnippets,/);
  assert.match(source, /options: \{[\s\S]*createHistory,[\s\S]*onEvent: channel,/);
  assert.doesNotMatch(source, /const ai(Stream)?Channel\s*=\s*new Channel/);
});

test('AI mock draft creates history only for a successful create-history request', async () => {
  await mockApi.clearAiHistory();
  await mockApi.saveAiProvider({
    id: 'draft-history-provider',
    name: 'Draft history',
    baseUrl: 'https://example.test/v1',
    model: 'draft-model',
    keyConfigured: true,
    protocol: 'responses',
    endpointMode: 'base',
    allowInsecureHttp: false,
  });

  await mockApi.continueAiConversation(
    'draft-history-provider',
    '',
    [],
    'compare draft evidence',
    [],
    [{ sourceName: 'worker.log', content: 'ERROR draft evidence' }],
    'draft-history-id',
    '2026-08-07T00:00:00Z',
    true,
  );
  const restored = await mockApi.loadAiHistory('draft-history-id');
  assert.equal(restored.providerId, 'draft-history-provider');
  assert.equal(restored.messages[0].content, 'compare draft evidence');
  assert.deepEqual(restored.messages[0].attachments, [
    { name: 'worker.log', charCount: 20, kind: 'selection' },
  ]);
  await assert.rejects(
    () =>
      mockApi.continueAiConversation(
        'draft-history-provider',
        '',
        [],
        'missing identity',
        [],
        [],
        undefined,
        undefined,
        true,
      ),
    /参数无效/,
  );
  await assert.rejects(
    () =>
      mockApi.continueAiConversation(
        'draft-history-provider',
        '',
        [{ role: 'user', content: 'old context' }],
        'must start empty',
        [],
        [],
        'another-draft-history',
        '2026-08-07T00:00:01Z',
        true,
      ),
    /参数无效/,
  );
  await assert.rejects(
    () =>
      mockApi.continueAiConversation(
        'draft-history-provider',
        '',
        [],
        'must not overwrite',
        [],
        [{ sourceName: 'worker.log', content: 'replacement' }],
        'draft-history-id',
        '2026-08-07T00:00:01Z',
        true,
      ),
    /已存在/,
  );
  assert.equal((await mockApi.loadAiHistory('draft-history-id')).messages.length, 2);
  await mockApi.deleteAiProvider('draft-history-provider');
  await mockApi.clearAiHistory();
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
    [],
    'history-with-attachment',
    '2026-08-06T00:00:01Z',
  );
  const restored = await mockApi.loadAiHistory('history-with-attachment');
  assert.equal(restored.updatedAt, '2026-08-06T00:00:01Z');
  assert.deepEqual(restored.messages[0].attachments, [
    { name: 'context.log', charCount: 0, kind: 'file' },
  ]);

  await mockApi.continueAiConversation(
    'history-provider',
    'ERROR original',
    restored.messages,
    '',
    [],
    [{ sourceName: 'worker.log', content: 'WARN selected context' }],
    'history-with-attachment',
    '2026-08-06T00:00:02Z',
  );
  const restoredWithSelection = await mockApi.loadAiHistory('history-with-attachment');
  assert.equal(restoredWithSelection.messages[2].content, '补充日志选区');
  assert.deepEqual(restoredWithSelection.messages[2].attachments, [
    { name: 'worker.log', charCount: 21, kind: 'selection' },
  ]);
  await assert.rejects(
    () =>
      mockApi.continueAiConversation(
        'history-provider',
        'x'.repeat(119_999),
        [],
        '',
        [],
        [{ sourceName: 'worker.log', content: 'xx' }],
      ),
    /合计超过 120000/,
  );
  await mockApi.deleteAiProvider('history-provider');
  await mockApi.clearAiHistory();
});
