import assert from 'node:assert/strict';
import test from 'node:test';
import { exportSnapshotAfterSelection } from './snapshotExport';

test('cancelling the save dialog does not export a snapshot', async () => {
  let exported = false;
  const result = await exportSnapshotAfterSelection(
    async () => null,
    async () => {
      exported = true;
      return { bytes: 1, complete: true };
    },
  );

  assert.equal(result, null);
  assert.equal(exported, false);
});

test('selected destination is forwarded and export failures remain visible', async () => {
  const result = await exportSnapshotAfterSelection(
    async () => 'D:\\saved.log',
    async (destination) => {
      assert.equal(destination, 'D:\\saved.log');
      return { bytes: 42, complete: false };
    },
  );
  assert.deepEqual(result, { bytes: 42, complete: false });

  await assert.rejects(
    exportSnapshotAfterSelection(
      async () => 'D:\\blocked.log',
      async () => {
        throw new Error('write denied');
      },
    ),
    /write denied/,
  );
});
