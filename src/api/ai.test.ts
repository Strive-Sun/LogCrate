import assert from 'node:assert/strict';
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
  const result = await mockApi.analyzeAiLog('analysis', 'ERROR failed');
  assert.equal(result.providerId, 'analysis');
  assert.match(result.content, /ERROR/);
  await mockApi.deleteAiProvider('analysis');
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
