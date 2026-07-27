import assert from 'node:assert/strict';
import test from 'node:test';
import type { FileSearchResult } from '../api/types';
import {
  mergeSearchResults,
  planSearchOpen,
  providerElapsedSeconds,
  searchPreparation,
  searchProgressRefreshKey,
  shouldApplySearchResponse,
} from './fileSearch';

test('索引进度分别格式化总耗时和当前阶段耗时', () => {
  assert.deepEqual(
    providerElapsedSeconds({
      root: 'C:\\',
      provider: 'windowsNtfs',
      phase: 'ready',
      elapsedMs: 12_300,
      stageElapsedMs: 400,
    }),
    { total: '12.3', stage: '0.4' },
  );
});

const result = (path: string): FileSearchResult => ({
  path,
  name: path.split(/[/\\]/).pop() ?? path,
  parent: path.replace(/[/\\][^/\\]*$/, ''),
  kind: 'log',
  size: 1,
  isLog: true,
  isArchive: false,
});

test('搜索结果打开动作区分日志、归档和普通文件', () => {
  assert.equal(
    planSearchOpen({
      path: '/tmp/app.log',
      name: 'app.log',
      watchPath: '/tmp',
      kind: 'file',
      isLog: true,
      alreadyMonitored: false,
    }),
    'log',
  );
  assert.equal(
    planSearchOpen({
      path: '/tmp/logs.zip',
      name: 'logs.zip',
      watchPath: '/tmp',
      kind: 'archive',
      isLog: false,
      alreadyMonitored: false,
    }),
    'archive',
  );
  assert.equal(
    planSearchOpen({
      path: '/tmp/report.pdf',
      name: 'report.pdf',
      watchPath: '/tmp',
      kind: 'file',
      isLog: false,
      alreadyMonitored: false,
    }),
    'reveal',
  );
});

test('快速连续输入只接受当前 generation 的响应', () => {
  assert.equal(shouldApplySearchResponse(7, 8), false);
  assert.equal(shouldApplySearchResponse(8, 8), true);
});

test('建索引期间按进度和逐卷完成状态刷新可见结果', () => {
  const status = {
    phase: 'scanning' as const,
    scannedFiles: 200_000,
    skippedDirectories: 0,
    indexedFiles: 99_999,
    indexBytes: 1,
    roots: ['C:\\', 'D:\\'],
    exclusions: [],
    providers: [
      { root: 'C:\\', provider: 'windowsNtfs', phase: 'scanning' },
      { root: 'D:\\', provider: 'windowsNtfs', phase: 'scanning' },
    ],
  };
  const initial = searchProgressRefreshKey(status);
  assert.equal(searchProgressRefreshKey({ ...status, indexedFiles: 100_000 }) === initial, false);
  assert.equal(
    searchProgressRefreshKey({
      ...status,
      providers: [status.providers[0], { ...status.providers[1], phase: 'ready' }],
    }) === initial,
    false,
  );
});

test('首个 MFT 批次前使用真实阶段而不生成零记录成果提示', () => {
  const preparation = searchPreparation({
    phase: 'scanning',
    scannedFiles: 0,
    skippedDirectories: 0,
    indexedFiles: 2_529_582,
    indexBytes: 1,
    roots: ['C:\\', 'D:\\'],
    exclusions: [],
    providers: [
      {
        root: 'C:\\',
        provider: 'windowsNtfs',
        phase: 'scanning',
        stage: 'readingUsn',
        discoveredRecords: 0,
      },
      {
        root: 'D:\\',
        provider: 'windowsNtfs',
        phase: 'scanning',
        stage: 'readingUsn',
        discoveredRecords: 0,
      },
    ],
  });
  assert.deepEqual(preparation, { roots: 'C:\\、D:\\', stage: 'readingUsn' });
});

test('分页追加去重并保持前端结果数量有界', () => {
  const merged = mergeSearchResults(
    [result('C:\\Logs\\app.log'), result('C:\\Logs\\server.log')],
    [result('c:/logs/APP.log'), result('C:\\Logs\\worker.log')],
    true,
    2,
  );
  assert.deepEqual(
    merged.map((item) => item.name),
    ['app.log', 'server.log'],
  );
});
