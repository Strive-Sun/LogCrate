import assert from 'node:assert/strict';
import test from 'node:test';
import { planFileDrop, singleDroppedPath } from './fileDrop';

test('singleDroppedPath accepts exactly one path and rejects a batch as a whole', () => {
  assert.equal(singleDroppedPath(['D:\\logs\\server.log']), 'D:\\logs\\server.log');
  assert.throws(() => singleDroppedPath([]), /fileDrop\.single/);
  assert.throws(() => singleDroppedPath(['one.log', 'two.log']), /fileDrop\.single/);
});

test('plain text opens immediately while its parent is added automatically', () => {
  assert.deepEqual(
    planFileDrop({
      path: 'D:\\logs\\server.json',
      name: 'server.json',
      kind: 'file',
      watchPath: 'D:\\logs',
      isLog: true,
      alreadyMonitored: false,
    }),
    {
      openPath: 'D:\\logs\\server.json',
      watchPathToAdd: 'D:\\logs',
      locateInTree: true,
    },
  );
});

test('covered archives reveal directly while new archive parents are added automatically', () => {
  const archive = {
    path: 'D:\\downloads\\logs.zip',
    name: 'logs.zip',
    kind: 'archive' as const,
    watchPath: 'D:\\downloads',
    isLog: false,
  };
  assert.deepEqual(planFileDrop({ ...archive, alreadyMonitored: true }), {
    openPath: null,
    watchPathToAdd: null,
    locateInTree: true,
  });
  assert.deepEqual(planFileDrop({ ...archive, alreadyMonitored: false }), {
    openPath: null,
    watchPathToAdd: 'D:\\downloads',
    locateInTree: true,
  });
});

test('covered plain text opens and locates without another monitoring addition', () => {
  assert.deepEqual(
    planFileDrop({
      path: 'D:\\logs\\nested\\server.log',
      name: 'server.log',
      kind: 'file',
      watchPath: 'D:\\logs\\nested',
      isLog: true,
      alreadyMonitored: true,
    }),
    {
      openPath: 'D:\\logs\\nested\\server.log',
      watchPathToAdd: null,
      locateInTree: true,
    },
  );
});

test('arbitrary files add their parent without opening and folders add themselves', () => {
  assert.deepEqual(
    planFileDrop({
      path: 'D:\\downloads\\image.png',
      name: 'image.png',
      kind: 'file',
      watchPath: 'D:\\downloads',
      isLog: false,
      alreadyMonitored: false,
    }),
    {
      openPath: null,
      watchPathToAdd: 'D:\\downloads',
      locateInTree: false,
    },
  );
  assert.deepEqual(
    planFileDrop({
      path: 'D:\\downloads\\logs',
      name: 'logs',
      kind: 'directory',
      watchPath: 'D:\\downloads\\logs',
      isLog: false,
      alreadyMonitored: false,
    }),
    {
      openPath: null,
      watchPathToAdd: 'D:\\downloads\\logs',
      locateInTree: false,
    },
  );
});
