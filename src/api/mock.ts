// 浏览器开发用的 mock 后端:模拟目录监控、免解压列条目、建索引、按行读取。
// 与 Tauri 后端实现同一套 API 契约(见 api/index.ts)。

import type {
  AppUpdateInfo,
  AppUpdateProgress,
  AiProviderConfig,
  AiAnalysisResult,
  ArchiveEntry,
  DirectoryChangeBatch,
  FileRevision,
  FileSearchConfig,
  FileSearchFeatureState,
  FileSearchFilter,
  FileSearchPage,
  FileSearchResult,
  FileSearchStatus,
  DroppedFileInfo,
  EncodingProgress,
  IndexProgress,
  LogLine,
  LogFieldAnchorResult,
  LogFieldFilterRequest,
  LogFieldLayout,
  LogFieldLayoutAnalysis,
  LogFieldMarkedLine,
  LogFieldProgress,
  LogFieldResultMode,
  LogFieldStatus,
  LogSearchRequest,
  LogSearchResult,
  MacOsFileAccessCapabilities,
  MacOsSystemSettingsResult,
  NewLogItem,
  OpenSessionResult,
  TreeNode,
} from './types';

let mockAiProviders: AiProviderConfig[] = [];

const LEVELS = ['INFO', 'INFO', 'DEBUG', 'WARN', 'INFO', 'ERROR', 'INFO', 'TRACE'];
const MSGS = [
  'service starting up, binding to 0.0.0.0:8080',
  'loading configuration from /etc/app/config.yaml',
  'established connection to database pool (size=16)',
  'incoming request GET /api/v1/users?page=3 latency=12ms',
  'cache miss for key user:8842, falling back to db',
  'unhandled rejection in worker #3: ETIMEDOUT after 30000ms',
  'flushed 2048 records to segment 0007, wal truncated',
  'retrying upstream call attempt=2 backoff=400ms',
];

const MOCK_SEARCH_RESULTS: FileSearchResult[] = [
  {
    path: 'D:\\project\\logs\\server-error.log',
    name: 'server-error.log',
    parent: 'D:\\project\\logs',
    kind: 'log',
    size: 28 * 1024 * 1024,
    modifiedMs: Date.now() - 20 * 60_000,
    isLog: true,
    isArchive: false,
  },
  {
    path: 'D:\\Downloads\\logs-0722.zip',
    name: 'logs-0722.zip',
    parent: 'D:\\Downloads',
    kind: 'archive',
    size: 96 * 1024 * 1024,
    modifiedMs: Date.now() - 90 * 60_000,
    isLog: false,
    isArchive: true,
  },
  {
    path: 'C:\\Users\\demo\\Desktop\\server.log',
    name: 'server.log',
    parent: 'C:\\Users\\demo\\Desktop',
    kind: 'log',
    size: 12 * 1024,
    modifiedMs: Date.now() - 24 * 60 * 60_000,
    isLog: true,
    isArchive: false,
  },
];

let mockSearchStatus: FileSearchStatus = {
  phase: 'ready',
  scannedFiles: 1_284_562,
  skippedDirectories: 3,
  indexedFiles: 1_284_562,
  indexBytes: 84 * 1024 * 1024,
  roots: ['C:\\', 'D:\\'],
  exclusions: [],
  providers: [
    { root: 'C:\\', provider: 'windowsNtfs', phase: 'ready' },
    { root: 'D:\\', provider: 'folderScan', phase: 'ready' },
  ],
};
let mockSearchFeatureState: FileSearchFeatureState = {
  currentEnabled: false,
  nextLaunchEnabled: false,
};
const mockSearchSubscribers = new Set<(status: FileSearchStatus) => void>();

function publishMockSearchStatus() {
  mockSearchSubscribers.forEach((subscriber) => subscriber(mockSearchStatus));
}

function pad(n: number, w: number) {
  return String(n).padStart(w, '0');
}

/** 稳定地为某个条目生成一行日志(纯函数,便于随机访问) */
function genLine(entrySeed: number, lineNo: number): string {
  const t = 10 * 3600 + entrySeed * 17 + lineNo; // 秒
  const hh = pad(Math.floor(t / 3600) % 24, 2);
  const mm = pad(Math.floor(t / 60) % 60, 2);
  const ss = pad(t % 60, 2);
  const lvl = LEVELS[(entrySeed + lineNo) % LEVELS.length];
  const msg = MSGS[(entrySeed * 3 + lineNo) % MSGS.length];
  let line = `2026-07-15 ${hh}:${mm}:${ss} ${lvl.padEnd(5)} [worker-${lineNo % 8}] ${msg} (seq=${lineNo})`;
  // 每隔一段插入一条超长行,演示横向滚动/截断
  if (lineNo % 500 === 0 && lineNo > 0) {
    line += ' ' + 'x'.repeat(2000) + ' <<超长行示例结束>>';
  }
  return line;
}

function mockWordCharacter(character: string | undefined): boolean {
  return character !== undefined && /[\p{L}\p{N}_]/u.test(character);
}

function mockLineMatch(
  line: string,
  request: LogSearchRequest,
  minimum: number,
  maximum: number,
): { startColumn: number; endColumn: number } | null {
  const source = request.caseSensitive ? line : line.toLocaleLowerCase();
  const query = request.caseSensitive ? request.query : request.query.toLocaleLowerCase();
  if (!query) return null;
  const candidates: Array<{ startColumn: number; endColumn: number }> = [];
  for (let start = source.indexOf(query); start >= 0; start = source.indexOf(query, start + 1)) {
    const end = start + query.length;
    if (start < minimum || (request.reverse ? end > maximum : start >= maximum)) continue;
    if (request.wholeWord && (mockWordCharacter(line[start - 1]) || mockWordCharacter(line[end]))) {
      continue;
    }
    candidates.push({ startColumn: start, endColumn: end });
  }
  return request.reverse ? (candidates.at(-1) ?? null) : (candidates[0] ?? null);
}

interface EntryMeta {
  seed: number;
  lineCount: number;
  entry: ArchiveEntry;
  /** 压缩条目需要后台解压建索引 */
  compressed: boolean;
}

// 每个条目路径 → 元信息
const ENTRY_TABLE: Record<string, EntryMeta> = {
  'crash-0715.zip::app.log': {
    seed: 1,
    lineCount: 5_000_000,
    compressed: true,
    entry: {
      path: 'app.log',
      size: 340 * 1024 * 1024,
      isLog: true,
      encrypted: false,
      isArchive: false,
    },
  },
  'crash-0715.zip::sys.log': {
    seed: 2,
    lineCount: 120_000,
    compressed: true,
    entry: {
      path: 'sys.log',
      size: 12 * 1024 * 1024,
      isLog: true,
      encrypted: false,
      isArchive: false,
    },
  },
  'crash-0715.zip::boot.txt': {
    seed: 3,
    lineCount: 8_400,
    compressed: true,
    entry: {
      path: 'boot.txt',
      size: 1.2 * 1024 * 1024,
      isLog: true,
      encrypted: false,
      isArchive: false,
    },
  },
  'crash-0715.zip::core.bin': {
    seed: 4,
    lineCount: 0,
    compressed: true,
    entry: {
      path: 'core.bin',
      size: 88 * 1024 * 1024,
      isLog: false,
      encrypted: false,
      isArchive: false,
    },
  },
  'server.log': {
    seed: 7,
    lineCount: 42_000,
    compressed: false,
    entry: {
      path: 'server.log',
      size: 6 * 1024 * 1024,
      isLog: true,
      encrypted: false,
      isArchive: false,
    },
  },
  'device3.zip::device.log': {
    seed: 9,
    lineCount: 260_000,
    compressed: true,
    entry: {
      path: 'device.log',
      size: 20 * 1024 * 1024,
      isLog: true,
      encrypted: false,
      isArchive: false,
    },
  },
};

const ARCHIVE_ENTRIES: Record<string, string[]> = {
  'crash-0715.zip': ['app.log', 'sys.log', 'boot.txt', 'core.bin'],
  'device3.zip': ['device.log'],
};

const progressTimers = new Map<string, number>();
let mockLogFieldGeneration = 0;
const mockLogFieldStatus = new Map<string, LogFieldStatus>();
const mockLogFieldSubscribers = new Map<string, Set<(progress: LogFieldProgress) => void>>();

const MOCK_LOG_FIELD_LAYOUT: LogFieldLayout = {
  fields: [
    {
      id: 'field-1',
      name: '时间',
      fieldType: 'time',
      boundary: { start: 0, end: 19 },
      displayWidth: 19,
    },
    {
      id: 'field-2',
      name: '级别',
      fieldType: 'level',
      boundary: { start: 20, end: 25 },
      displayWidth: 5,
    },
    {
      id: 'field-3',
      name: '正文',
      fieldType: 'text',
      boundary: { start: 26, end: null },
      displayWidth: 48,
    },
  ],
  pattern: { kind: 'manualColumns' },
  confidence: 1,
  source: 'automatic',
};
let encodingGeneration = 0;
const encodingByKey = new Map<string, string>();

export const mockApi = {
  async listAiProviders(): Promise<AiProviderConfig[]> {
    return mockAiProviders.map((provider) => ({ ...provider }));
  },
  async saveAiProvider(config: AiProviderConfig, apiKey?: string): Promise<AiProviderConfig> {
    const saved = { ...config, keyConfigured: Boolean(apiKey?.trim()) || config.keyConfigured };
    mockAiProviders = [...mockAiProviders.filter((item) => item.id !== config.id), saved];
    return { ...saved };
  },
  async deleteAiProvider(providerId: string): Promise<void> {
    mockAiProviders = mockAiProviders.filter((item) => item.id !== providerId);
  },
  async testAiProvider(providerId: string): Promise<void> {
    const provider = mockAiProviders.find((item) => item.id === providerId);
    if (!provider?.keyConfigured) throw new Error('AI provider API key is not configured');
  },
  async analyzeAiLog(providerId: string, selectedText: string): Promise<AiAnalysisResult> {
    const provider = mockAiProviders.find((item) => item.id === providerId);
    if (!provider) throw new Error('AI provider was not found');
    if (!selectedText.trim()) throw new Error('Select some log text before starting AI analysis');
    return {
      providerId,
      model: provider.model,
      content: `主要信息：选中的日志共 ${selectedText.length} 个字符。\n\n警告：请结合上下文进一步确认。\n\n错误：未发现可由 mock 确认的错误。\n\n建议：检查 ERROR/WARN 行及其前后文。`,
    };
  },
  async fileRevision(_path: string): Promise<FileRevision> {
    return { exists: true, revision: 'mock:1' };
  },
  async setAppLocale(_locale: 'zh-CN' | 'en'): Promise<void> {},
  async getAppVersion(): Promise<string> {
    return '1.0.1';
  },

  async checkForUpdate(): Promise<AppUpdateInfo | null> {
    await delay(350);
    return {
      currentVersion: '1.0.1',
      version: '1.1.0',
      date: '2026-07-18T00:00:00Z',
      body: '浏览器 mock：演示新版本下载与安装流程。',
    };
  },

  async downloadAndInstallUpdate(onProgress: (progress: AppUpdateProgress) => void): Promise<void> {
    const totalBytes = 10 * 1024 * 1024;
    for (let percent = 0; percent <= 100; percent += 10) {
      await delay(80);
      onProgress({
        phase: percent === 100 ? 'installing' : 'downloading',
        downloadedBytes: Math.round((totalBytes * percent) / 100),
        totalBytes,
        percent,
      });
    }
  },

  async discardPendingUpdate(): Promise<void> {},

  async listWatchDirs(): Promise<TreeNode[]> {
    return [
      {
        id: 'dir:downloads',
        name: '下载',
        kind: 'dir',
        watchRoot: true,
        children: [
          {
            id: 'arc:crash-0715.zip',
            name: 'crash-0715.zip',
            kind: 'archive',
            size: 96 * 1024 * 1024,
            isLog: true,
            watchDir: '下载',
            unread: true,
          },
          {
            id: 'file:server.log',
            name: 'server.log',
            kind: 'file',
            size: 6 * 1024 * 1024,
            isLog: true,
            watchDir: '下载',
            unread: true,
          },
        ],
      },
      {
        id: 'dir:backup',
        name: '日志备份',
        kind: 'dir',
        watchRoot: true,
        children: [
          {
            id: 'arc:device3.zip',
            name: 'device3.zip',
            kind: 'archive',
            size: 30 * 1024 * 1024,
            isLog: true,
            watchDir: '日志备份',
            unread: true,
          },
        ],
      },
    ];
  },

  async listArchiveEntries(archiveName: string): Promise<ArchiveEntry[]> {
    // 模拟“只读中央目录”的极短延迟
    await delay(120);
    const names = ARCHIVE_ENTRIES[archiveName] ?? [];
    return names.map((n) => ENTRY_TABLE[`${archiveName}::${n}`].entry);
  },

  async expandDirectory(_path: string): Promise<TreeNode[]> {
    return [];
  },

  async collapseDirectory(_path: string): Promise<void> {},

  async newLogItems(): Promise<NewLogItem[]> {
    return [
      {
        id: 'arc:crash-0715.zip',
        name: 'crash-0715.zip',
        kind: 'archive',
        source: '下载',
        age: '2m',
      },
      { id: 'file:server.log', name: 'server.log', kind: 'file', source: '下载', age: '5m' },
      {
        id: 'arc:device3.zip',
        name: 'device3.zip',
        kind: 'archive',
        source: '日志备份',
        age: '10m',
      },
    ];
  },

  async openLogSession(entryKey: string): Promise<OpenSessionResult> {
    const meta = ENTRY_TABLE[entryKey];
    if (!meta) throw new Error(`条目不存在: ${entryKey}`);
    if (!meta.entry.isLog) throw new Error('该条目不是文本日志,无法查看');
    return {
      sessionId: `sess:${entryKey}`,
      sourcePath: entryKey.split('::', 1)[0],
      entryPath: entryKey.replace('::', ' › '),
      size: meta.entry.size,
      indexing: meta.compressed && meta.lineCount > 300_000,
      encoding: 'UTF-8',
      evictedSessionIds: [],
    };
  },

  async closeLogSession(entryKey: string, _expectedSessionId?: string): Promise<void> {
    mockLogFieldStatus.delete(entryKey);
    mockLogFieldSubscribers.delete(entryKey);
  },

  async saveSessionSnapshot(
    entryKey: string,
    _suggestedName: string,
    _title: string,
  ): Promise<{ bytes: number; complete: boolean } | null> {
    const meta = ENTRY_TABLE[entryKey];
    if (!meta) throw new Error(`条目不存在: ${entryKey}`);
    return { bytes: meta.entry.size, complete: true };
  },

  /** 模拟后台建索引进度;返回取消函数 */
  subscribeIndexProgress(
    entryKey: string,
    onProgress: (p: IndexProgress) => void,
    onDone: (totalLines: number) => void,
  ): () => void {
    const meta = ENTRY_TABLE[entryKey];
    const total = meta?.lineCount ?? 0;
    const previousTimer = progressTimers.get(entryKey);
    if (previousTimer) window.clearInterval(previousTimer);
    let percent = 0;
    const timer = window.setInterval(() => {
      percent += 7 + Math.floor(percent / 20);
      if (percent >= 100) {
        percent = 100;
        onProgress({
          sessionId: `sess:${entryKey}`,
          percent,
          indexedLines: total,
          done: true,
          failed: false,
          detectedEncoding: 'UTF-8',
          effectiveEncoding: encodingByKey.get(entryKey) ?? 'UTF-8',
        });
        onDone(total);
        window.clearInterval(timer);
        progressTimers.delete(entryKey);
        return;
      }
      onProgress({
        sessionId: `sess:${entryKey}`,
        percent,
        indexedLines: Math.floor((total * percent) / 100),
        done: false,
        failed: false,
        detectedEncoding: 'UTF-8',
        effectiveEncoding: encodingByKey.get(entryKey) ?? 'UTF-8',
      });
    }, 180);
    progressTimers.set(entryKey, timer);
    return () => {
      window.clearInterval(timer);
      if (progressTimers.get(entryKey) === timer) progressTimers.delete(entryKey);
    };
  },

  async readLines(entryKey: string, start: number, count: number): Promise<LogLine[]> {
    const meta = ENTRY_TABLE[entryKey];
    if (!meta) return [];
    const end = Math.min(start + count, meta.lineCount);
    const out: LogLine[] = [];
    for (let i = start; i < end; i++) {
      const raw = genLine(meta.seed, i);
      const truncated = raw.length > 1024;
      out.push({
        lineNo: i + 1,
        content: truncated ? raw.slice(0, 1024) : raw,
        truncated,
      });
    }
    return out;
  },

  async analyzeLogFieldLayout(
    entryKey: string,
    phase: 'quick' | 'background',
  ): Promise<LogFieldLayoutAnalysis> {
    const total = ENTRY_TABLE[entryKey]?.lineCount ?? 0;
    const sampledNonEmptyLines = Math.min(total, phase === 'quick' ? 256 : 10_000);
    return {
      layout: structuredClone(MOCK_LOG_FIELD_LAYOUT),
      sampledNonEmptyLines,
      sampledBytes: sampledNonEmptyLines * 96,
      mainLayoutLines: sampledNonEmptyLines,
      unparsedLines: 0,
    };
  },

  async setLogFieldFilter(entryKey: string, request: LogFieldFilterRequest): Promise<number> {
    const totalLines = ENTRY_TABLE[entryKey]?.lineCount ?? 0;
    const generation = ++mockLogFieldGeneration;
    const status: LogFieldStatus = {
      generation,
      layout: request.layout,
      conditions: request.conditions,
      statistics: [],
      scannedLines: totalLines,
      matchedLines: totalLines,
      unparsedLines: 0,
      totalLines,
      done: true,
      failed: false,
    };
    mockLogFieldStatus.set(entryKey, status);
    window.setTimeout(() => {
      const progress: LogFieldProgress = { sessionId: `sess:${entryKey}`, ...status };
      mockLogFieldSubscribers.get(entryKey)?.forEach((subscriber) => subscriber(progress));
    }, 0);
    return generation;
  },

  async logFieldStatus(entryKey: string): Promise<LogFieldStatus | null> {
    return mockLogFieldStatus.get(entryKey) ?? null;
  },

  async clearLogFieldFilter(entryKey: string): Promise<void> {
    mockLogFieldStatus.delete(entryKey);
  },

  async readFilteredLines(
    entryKey: string,
    _generation: number,
    start: number,
    count: number,
    _includeUnparsed: boolean,
  ): Promise<LogLine[]> {
    return this.readLines(entryKey, start, count);
  },

  async readLinesWithFieldMatches(
    entryKey: string,
    _generation: number,
    start: number,
    count: number,
  ): Promise<LogFieldMarkedLine[]> {
    return (await this.readLines(entryKey, start, count)).map((line) => ({
      ...line,
      fieldMatched: true,
      fieldUnparsed: false,
    }));
  },

  async locateLogFieldAnchor(
    entryKey: string,
    _generation: number,
    originalLineNo: number,
    _mode: LogFieldResultMode,
    _includeUnparsed: boolean,
  ): Promise<LogFieldAnchorResult | null> {
    const total = ENTRY_TABLE[entryKey]?.lineCount ?? 0;
    if (total === 0) return null;
    const lineNo = Math.min(Math.max(1, originalLineNo), total);
    return { viewIndex: lineNo - 1, lineNo };
  },

  subscribeLogFieldProgress(
    entryKey: string,
    generation: number,
    onProgress: (progress: LogFieldProgress) => void,
  ): () => void {
    const subscriber = (progress: LogFieldProgress) => {
      if (progress.generation === generation) onProgress(progress);
    };
    const subscribers = mockLogFieldSubscribers.get(entryKey) ?? new Set();
    subscribers.add(subscriber);
    mockLogFieldSubscribers.set(entryKey, subscribers);
    const status = mockLogFieldStatus.get(entryKey);
    if (status?.generation === generation) {
      window.setTimeout(() => subscriber({ sessionId: `sess:${entryKey}`, ...status }), 0);
    }
    return () => {
      const current = mockLogFieldSubscribers.get(entryKey);
      current?.delete(subscriber);
      if (current?.size === 0) mockLogFieldSubscribers.delete(entryKey);
    };
  },

  async searchLog(entryKey: string, request: LogSearchRequest): Promise<LogSearchResult> {
    const meta = ENTRY_TABLE[entryKey];
    if (!meta) throw new Error('session not found');
    if (!request.query) throw new Error('search query cannot be empty');
    const indexedLines = meta.lineCount;
    if (indexedLines === 0) {
      return {
        match: null,
        wrapped: false,
        reachedBoundary: true,
        indexedLines,
        indexing: false,
      };
    }
    const startLine = Math.min(Math.max(0, request.startLine), indexedLines - 1);
    const scan = async (
      from: number,
      to: number,
      wrapped: boolean,
    ): Promise<LogSearchResult['match']> => {
      const step = request.reverse ? -1 : 1;
      for (let lineNo = from; request.reverse ? lineNo >= to : lineNo <= to; lineNo += step) {
        const line = genLine(meta.seed, lineNo);
        const isCursorLine = lineNo === startLine;
        const cursor = request.startColumn ?? (request.reverse ? line.length : 0);
        const minimum =
          isCursorLine && ((request.reverse && wrapped) || (!request.reverse && !wrapped))
            ? cursor
            : 0;
        const maximum =
          isCursorLine && ((request.reverse && !wrapped) || (!request.reverse && wrapped))
            ? cursor
            : Number.POSITIVE_INFINITY;
        const matched = mockLineMatch(line, request, minimum, maximum);
        if (matched) return { lineNo: lineNo + 1, ...matched };
        if (Math.abs(lineNo - from) % 2000 === 1999) await delay(0);
      }
      return null;
    };

    const first = request.reverse
      ? await scan(startLine, 0, false)
      : await scan(startLine, indexedLines - 1, false);
    if (first || !request.wrap) {
      return {
        match: first,
        wrapped: false,
        reachedBoundary: first === null,
        indexedLines,
        indexing: meta.compressed && meta.lineCount > 300_000,
      };
    }
    const wrapped = request.reverse
      ? await scan(indexedLines - 1, startLine, true)
      : await scan(0, startLine, true);
    return {
      match: wrapped,
      wrapped: true,
      reachedBoundary: wrapped === null,
      indexedLines,
      indexing: meta.compressed && meta.lineCount > 300_000,
    };
  },

  lineCount(entryKey: string): number {
    return ENTRY_TABLE[entryKey]?.lineCount ?? 0;
  },

  async setSessionEncoding(entryKey: string, encoding: string): Promise<number> {
    encodingByKey.set(entryKey, encoding);
    encodingGeneration += 1;
    return encodingGeneration;
  },

  subscribeEncodingProgress(
    entryKey: string,
    generation: number,
    onProgress: (progress: EncodingProgress) => void,
  ): () => void {
    const timer = window.setTimeout(() => {
      onProgress({
        sessionId: `sess:${entryKey}`,
        generation,
        percent: 100,
        encoding: encodingByKey.get(entryKey) ?? 'UTF-8',
        lineCount: ENTRY_TABLE[entryKey]?.lineCount ?? 0,
        done: true,
        failed: false,
      });
    }, 120);
    return () => window.clearTimeout(timer);
  },

  async addWatchDir(_title?: string): Promise<boolean> {
    throw new Error('mock.selectDirectory');
  },

  async macOsFileAccessCapabilities(): Promise<MacOsFileAccessCapabilities> {
    return { supported: false, onboardingVersion: 1, sandboxed: false };
  },

  async openMacOsFullDiskAccessSettings(): Promise<MacOsSystemSettingsResult> {
    throw new Error('MACOS_ONLY');
  },

  async reauthorizeWatchDir(_existingPath: string, _title?: string): Promise<boolean> {
    throw new Error('mock.selectDirectory');
  },

  async inspectDroppedFile(_path: string): Promise<DroppedFileInfo> {
    throw new Error('mock.dropUnsupported');
  },

  async fileSearchStatus(): Promise<FileSearchStatus> {
    return mockSearchStatus;
  },

  async fileSearchConfig(): Promise<FileSearchConfig> {
    return {
      version: 1,
      enabled: mockSearchStatus.phase !== 'disabled',
      roots: mockSearchStatus.roots,
      exclusions: mockSearchStatus.exclusions,
    };
  },

  async fileSearchFeatureState(): Promise<FileSearchFeatureState> {
    return mockSearchFeatureState;
  },

  async setFileSearchEnabled(enabled: boolean): Promise<FileSearchFeatureState> {
    mockSearchFeatureState = { ...mockSearchFeatureState, nextLaunchEnabled: enabled };
    return mockSearchFeatureState;
  },

  async startFileSearchIndex(_rebuild = false): Promise<void> {
    mockSearchStatus = { ...mockSearchStatus, phase: 'scanning', scannedFiles: 428_731 };
    publishMockSearchStatus();
    window.setTimeout(() => {
      mockSearchStatus = { ...mockSearchStatus, phase: 'ready', scannedFiles: 1_284_562 };
      publishMockSearchStatus();
    }, 800);
  },

  async pauseFileSearchIndex(): Promise<void> {
    mockSearchStatus = { ...mockSearchStatus, phase: 'paused' };
    publishMockSearchStatus();
  },

  async clearFileSearchIndex(): Promise<void> {
    mockSearchStatus = {
      ...mockSearchStatus,
      phase: 'disabled',
      scannedFiles: 0,
      indexedFiles: 0,
      indexBytes: 0,
    };
    publishMockSearchStatus();
  },

  async chooseFileSearchExclusion(_title?: string): Promise<string | null> {
    throw new Error('mock.selectDirectory');
  },

  async setFileSearchExclusions(exclusions: string[]): Promise<void> {
    mockSearchStatus = { ...mockSearchStatus, exclusions, phase: 'scanning' };
    publishMockSearchStatus();
  },

  async repairFileSearchService(): Promise<void> {},

  async searchFiles(
    query: string,
    filter: FileSearchFilter,
    offset = 0,
    limit = 200,
  ): Promise<FileSearchPage> {
    await delay(20);
    const terms = query.toLocaleLowerCase().split(/\s+/).filter(Boolean);
    const matches = MOCK_SEARCH_RESULTS.filter((item) => {
      if (filter === 'log' && item.kind !== 'log') return false;
      if (filter === 'archive' && item.kind !== 'archive') return false;
      const haystack = `${item.name} ${item.path}`.toLocaleLowerCase();
      return terms.every((term) => haystack.includes(term));
    });
    return {
      items: matches.slice(offset, offset + limit),
      total: matches.length,
      partial: mockSearchStatus.phase === 'scanning',
      elapsedMs: 18,
    };
  },

  async inspectSearchResult(path: string): Promise<DroppedFileInfo> {
    const item = MOCK_SEARCH_RESULTS.find((candidate) => candidate.path === path);
    if (!item) throw new Error('文件已被删除或移动');
    return {
      path: item.path,
      name: item.name,
      kind: item.isArchive ? 'archive' : 'file',
      watchPath: item.parent,
      isLog: item.isLog,
      alreadyMonitored: false,
    };
  },

  async addSearchResultParent(path: string): Promise<string> {
    const item = MOCK_SEARCH_RESULTS.find((candidate) => candidate.path === path);
    if (!item) throw new Error('文件已被删除或移动');
    return item.parent;
  },

  subscribeFileSearchStatus(onStatus: (status: FileSearchStatus) => void): () => void {
    mockSearchSubscribers.add(onStatus);
    return () => mockSearchSubscribers.delete(onStatus);
  },

  async addWatchPath(_path: string): Promise<void> {},

  async removeWatchDir(_dirPath: string): Promise<void> {},

  async renameFile(path: string, newName: string): Promise<string> {
    const parent = path.replace(/[/\\][^/\\]*$/, '');
    return `${parent}/${newName}`;
  },

  async deleteFile(_path: string): Promise<void> {},

  async openPath(_path: string): Promise<void> {
    throw new Error('mock.fileManager');
  },

  async renameWatchDir(path: string, newName: string): Promise<string> {
    const parent = path.replace(/[/\\][^/\\]*$/, '');
    return `${parent}/${newName}`;
  },

  async deleteWatchDir(_path: string): Promise<void> {},

  async setFilter(_suffixes: string[], _showAll: boolean): Promise<void> {},

  async getFilter(): Promise<[string[], boolean]> {
    return [['.log', '.txt', '.out'], false];
  },

  subscribeNewLogs(_onDetect: (item: NewLogItem) => void): () => void {
    return () => {};
  },

  subscribeDirectoryChanges(_onChange: (batch: DirectoryChangeBatch) => void): () => void {
    return () => {};
  },
};

function delay(ms: number) {
  return new Promise((r) => setTimeout(r, ms));
}
