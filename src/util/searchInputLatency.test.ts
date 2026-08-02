import assert from 'node:assert/strict';
import test from 'node:test';
import { recordSearchInputLatency, resetSearchInputLatencySamples } from './searchInputLatency';

test('search input latency keeps bounded raw samples and computes p95', () => {
  resetSearchInputLatencySamples();
  const startedAt = performance.now() - 8;
  const report = recordSearchInputLatency(startedAt, 'scanning');
  assert.equal(report.sampleCount, 1);
  assert.ok(report.p95Ms >= 7);
  assert.deepEqual(report.samplesMs.length, 1);
  assert.equal(report.phase, 'scanning');
});
