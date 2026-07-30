import assert from 'node:assert/strict';
import test from 'node:test';
import {
  FileSearchServiceRepairError,
  normalizeFileSearchServiceRepairError,
} from './fileSearchServiceRepair';

test('结构化、旧字符串和未知修复错误都归一化为稳定代码', () => {
  const structured = normalizeFileSearchServiceRepairError({
    code: 'accessDenied',
    message: 'Win32 5',
  });
  assert.equal(structured.code, 'accessDenied');
  assert.equal(structured.message, 'Win32 5');

  const legacy = normalizeFileSearchServiceRepairError('[protocolMismatch] protocol 1');
  assert.equal(legacy.code, 'protocolMismatch');
  assert.equal(legacy.message, 'protocol 1');

  const unknown = normalizeFileSearchServiceRepairError(new Error('bridge failed'));
  assert.equal(unknown.code, 'repairFailed');
  assert.equal(unknown.message, 'bridge failed');

  assert.equal(
    normalizeFileSearchServiceRepairError(structured),
    structured,
    'normalized errors remain idempotent',
  );
  assert.ok(structured instanceof FileSearchServiceRepairError);
});
