import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import type { DirectoryChangeBatch, TreeNode } from '../api/types';
import {
  applyDirectoryChanges,
  findTreeNode,
  isActiveTreeNode,
  isPathInsideDirectory,
  passesDirectoryFilter,
  revealDirectoryChain,
  removedDirectoryNodes,
} from './directoryTree';

const file = (id: string, name: string, isLog = true): TreeNode => ({
  id,
  path: id,
  name,
  kind: 'file',
  isLog,
});

const base: TreeNode[] = [
  {
    id: 'C:\\logs',
    path: 'C:\\logs',
    name: 'logs',
    kind: 'dir',
    watchRoot: true,
    children: [file('C:\\logs\\b.log', 'b.log')],
  },
  { id: 'C:\\other', path: 'C:\\other', name: 'other', kind: 'dir', children: [] },
];

function apply(changes: DirectoryChangeBatch['changes']) {
  return applyDirectoryChanges(base, { watchDir: 'C:\\logs', changes });
}

describe('目录树增量同步', () => {
  it('支持新增、修改和稳定排序，同时保留其它目录引用', () => {
    const next = apply([
      { type: 'upsert', node: file('C:\\logs\\a.log', 'a.log') },
      { type: 'upsert', node: { ...file('C:\\logs\\b.log', 'b.log'), size: 42 } },
    ]);
    assert.deepEqual(
      next[0].children?.map((node) => node.name),
      ['a.log', 'b.log'],
    );
    assert.equal(next[0].children?.[1].size, 42);
    assert.equal(next[1], base[1]);
  });

  it('支持删除、重命名和 rescan 局部替换', () => {
    const renamed = apply([
      {
        type: 'rename',
        oldPath: 'C:\\logs\\b.log',
        node: file('C:\\logs\\c.log', 'c.log'),
      },
    ]);
    assert.deepEqual(
      renamed[0].children?.map((node) => node.name),
      ['c.log'],
    );
    assert.deepEqual(
      removedDirectoryNodes(base, renamed, 'C:\\logs').map((node) => node.name),
      ['b.log'],
    );

    const rescanned = applyDirectoryChanges(renamed, {
      watchDir: 'C:\\logs',
      changes: [{ type: 'rescan', nodes: [file('C:\\logs\\z.bin', 'z.bin', false)] }],
    });
    assert.deepEqual(
      rescanned[0].children?.map((node) => node.name),
      ['z.bin'],
    );
  });

  it('隐藏文件持续保留在库存中，切换显示全部后立即可见', () => {
    const hidden = file('C:\\logs\\dump.bin', 'dump.bin', false);
    const next = apply([{ type: 'upsert', node: hidden }]);
    assert.ok(next[0].children?.some((node) => node.id === hidden.id));
    assert.equal(passesDirectoryFilter(hidden, ['.log'], false), false);
    assert.equal(passesDirectoryFilter(hidden, ['.log'], true), true);
  });

  it('递归定位已展开目录，并只更新对应分支', () => {
    const nested: TreeNode = {
      id: 'C:\\logs\\nested',
      path: 'C:\\logs\\nested',
      name: 'nested',
      kind: 'dir',
      children: [],
    };
    const tree: TreeNode[] = [{ ...base[0], children: [nested] }, base[1]];
    const next = applyDirectoryChanges(tree, {
      watchDir: nested.id,
      changes: [{ type: 'upsert', node: file('C:\\logs\\nested\\child.log', 'child.log') }],
    });

    assert.equal(next[0].children?.[0].children?.[0].name, 'child.log');
    assert.equal(next[1], tree[1]);
  });

  it('目录优先于文件排序，rescan 保留已加载的后代', () => {
    const directory: TreeNode = {
      id: 'C:\\logs\\folder',
      path: 'C:\\logs\\folder',
      name: 'folder',
      kind: 'dir',
      children: [file('C:\\logs\\folder\\kept.log', 'kept.log')],
    };
    const tree: TreeNode[] = [{ ...base[0], children: [directory] }, base[1]];
    const next = applyDirectoryChanges(tree, {
      watchDir: 'C:\\logs',
      changes: [
        {
          type: 'rescan',
          nodes: [file('C:\\logs\\a.log', 'a.log'), { ...directory, children: undefined }],
        },
      ],
    });

    assert.deepEqual(
      next[0].children?.map((node) => node.name),
      ['folder', 'a.log'],
    );
    assert.equal(next[0].children?.[0].children?.[0].name, 'kept.log');
  });

  it('为直属和深层通知计算逐层加载链', () => {
    assert.deepEqual(revealDirectoryChain(base, 'C:\\logs\\direct.zip'), ['C:\\logs']);
    assert.deepEqual(revealDirectoryChain(base, 'C:\\logs\\a\\b\\deep.zip'), [
      'C:\\logs',
      'C:\\logs\\a',
      'C:\\logs\\a\\b',
    ]);
  });

  it('重叠根选择最具体目录，并兼容 Windows 混合分隔符和大小写', () => {
    const tree: TreeNode[] = [
      { ...base[0], watchRoot: true },
      {
        id: 'C:\\logs\\nested',
        path: 'C:\\logs\\nested',
        name: 'nested',
        kind: 'dir',
        watchRoot: true,
      },
    ];
    assert.deepEqual(revealDirectoryChain(tree, 'c:/LOGS/NESTED/day/result.zip'), [
      'C:\\logs\\nested',
      'C:\\logs\\nested\\day',
    ]);
  });

  it('支持 macOS 路径并拒绝监控根之外的目标', () => {
    const tree: TreeNode[] = [
      {
        id: '/Users/test/logs',
        path: '/Users/test/logs',
        name: 'logs',
        kind: 'dir',
        watchRoot: true,
      },
    ];
    assert.deepEqual(revealDirectoryChain(tree, '/Users/test/logs/day/result.log'), [
      '/Users/test/logs',
      '/Users/test/logs/day',
    ]);
    assert.deepEqual(revealDirectoryChain(tree, '/Users/test/other/result.log'), []);
    assert.equal(findTreeNode(base, 'C:\\logs\\b.log')?.name, 'b.log');
  });

  it('按路径边界判断折叠目录是否包含未读文件', () => {
    assert.equal(isPathInsideDirectory('d:/LOGS/day/new.zip', 'D:\\logs'), true);
    assert.equal(isPathInsideDirectory('D:\\logs-other\\new.zip', 'D:\\logs'), false);
    assert.equal(isPathInsideDirectory('D:\\logs', 'D:\\logs'), false);
    assert.equal(isPathInsideDirectory('/Users/test/logs/day/new.log', '/Users/test/logs'), true);
    assert.equal(isPathInsideDirectory('/Users/test/Logs/day/new.log', '/Users/test/logs'), false);
  });

  it('当前查看文件即使不匹配后缀筛选也保持可见', () => {
    const source = file('D:\\logs\\src\\napi_class.cc', 'napi_class.cc');
    const archive: TreeNode = {
      id: 'D:\\logs\\bundle.zip',
      path: 'D:\\logs\\bundle.zip',
      name: 'bundle.zip',
      kind: 'archive',
    };
    assert.equal(passesDirectoryFilter(source, ['.log'], false), false);
    assert.equal(isActiveTreeNode(source, source.id, null), true);
    assert.equal(isActiveTreeNode(source, null, null), false);
    assert.equal(isActiveTreeNode(archive, null, archive.id), true);
  });
});
