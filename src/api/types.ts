// 与技术设计文档 4.3 / 4.6 的后端契约对齐的前端类型

/** 归档内的一个条目(不含内容,仅元信息) */
export interface ArchiveEntry {
  /** 包内路径 */
  path: string;
  /** 解压后大小(字节) */
  size: number;
  /** 是否日志/文本 */
  isLog: boolean;
  /** 是否加密条目(M1 不支持,列出但标记) */
  encrypted: boolean;
  /** 是否为可继续惰性展开的嵌套归档 */
  isArchive: boolean;
}

/** 一行返回给前端的内容 */
export interface LogLine {
  /** 行号(从 1 起) */
  lineNo: number;
  /** 已解码为 UTF-8 的行内容(可能被截断) */
  content: string;
  /** 是否因超过阈值被后端截断 */
  truncated: boolean;
}

export interface LogSearchRequest {
  query: string;
  /** Zero-based line used as the first search position. */
  startLine: number;
  /** UTF-16 code-unit offset; omitted means line start forward and line end in reverse. */
  startColumn?: number;
  reverse: boolean;
  wholeWord: boolean;
  caseSensitive: boolean;
  wrap: boolean;
  fieldView?: LogFieldSearchView;
}

export interface LogSearchMatch {
  /** One-based line number, matching LogLine.lineNo. */
  lineNo: number;
  /** UTF-16 code-unit offsets for direct use with JavaScript string slicing. */
  startColumn: number;
  endColumn: number;
}

export interface LogSearchResult {
  match: LogSearchMatch | null;
  wrapped: boolean;
  reachedBoundary: boolean;
  indexedLines: number;
  indexing: boolean;
}

export type LogFieldType = 'time' | 'level' | 'discrete' | 'text';
export type LogFieldResultMode = 'compact' | 'highlight';

export interface LogFieldDefinition {
  id: string;
  name: string;
  fieldType: LogFieldType;
  boundary: { start: number; end: number | null };
  displayWidth: number;
}

export type LogFieldLayoutPattern =
  | { kind: 'bracketed'; segmentCount: number }
  | { kind: 'chromium' }
  | { kind: 'androidLogcat' }
  | { kind: 'manualColumns' };

export interface LogFieldLayout {
  fields: LogFieldDefinition[];
  pattern: LogFieldLayoutPattern;
  confidence: number;
  source: 'automatic' | 'manual';
}

export interface LogFieldLayoutAnalysis {
  layout: LogFieldLayout | null;
  sampledNonEmptyLines: number;
  sampledBytes: number;
  mainLayoutLines: number;
  unparsedLines: number;
}

export type LogFieldCondition =
  | { kind: 'discrete'; fieldId: string; values: string[] }
  | { kind: 'time'; fieldId: string; start?: string; end?: string }
  | { kind: 'text'; fieldId: string; query: string; caseSensitive: boolean };

export interface LogFieldFilterRequest {
  layout: LogFieldLayout;
  conditions: LogFieldCondition[];
}

export interface LogFieldCandidateValue {
  value: string;
  count: number;
}

export interface LogFieldStatistics {
  fieldId: string;
  candidates: LogFieldCandidateValue[];
  highCardinality: boolean;
  minTime?: string;
  maxTime?: string;
}

export interface LogFieldProgress {
  sessionId: string;
  generation: number;
  scannedLines: number;
  matchedLines: number;
  unparsedLines: number;
  totalLines: number;
  done: boolean;
  failed: boolean;
  error?: string;
}

export interface LogFieldStatus extends Omit<LogFieldProgress, 'sessionId'> {
  layout: LogFieldLayout;
  conditions: LogFieldCondition[];
  statistics: LogFieldStatistics[];
}

export interface LogFieldMarkedLine extends LogLine {
  fieldMatched: boolean;
  fieldUnparsed: boolean;
}

export interface LogFieldAnchorResult {
  viewIndex: number;
  lineNo: number;
}

export interface LogFieldSearchView {
  generation: number;
  mode: LogFieldResultMode;
  includeUnparsed: boolean;
}

/** 监控目录树中的节点类型 */
export type NodeKind = 'dir' | 'archive' | 'file';

/** 目录树节点 */
export interface TreeNode {
  id: string;
  name: string;
  kind: NodeKind;
  /** 文件/条目大小(字节),目录为 undefined */
  size?: number;
  /** 是否日志文件(archive 节点恒为 true;file 视扩展名/采样) */
  isLog?: boolean;
  /** 磁盘绝对路径(用于重命名/删除等文件操作) */
  path?: string;
  /** 来源监控目录路径 */
  watchDir?: string;
  /** 是否为用户配置的监控根目录；普通子目录为 false。 */
  watchRoot?: boolean;
  /** macOS 持久文件访问状态；其它平台通常为 available。 */
  accessStatus?: 'available' | 'needsAuthorization' | 'unavailable';
  /** 是否为未读的新到达项 */
  unread?: boolean;
  /** 子节点;archive 节点在展开时惰性填充 */
  children?: TreeNode[];
}

export interface MacOsFileAccessCapabilities {
  supported: boolean;
  onboardingVersion: number;
  sandboxed: boolean;
}

export interface MacOsSystemSettingsResult {
  usedFallback: boolean;
}

/** 新日志提示项 */
export interface NewLogItem {
  id: string;
  name: string;
  kind: 'archive' | 'file';
  /** 来源目录短名 */
  source: string;
  /** 到达距今(如 "2m") */
  age: string;
}

/** 建索引进度事件载荷 */
export interface IndexProgress {
  sessionId: string;
  percent: number;
  indexedLines: number;
  done: boolean;
  failed: boolean;
  detectedEncoding: string;
  effectiveEncoding: string;
  error?: string;
}

/** 手动编码重建进度事件载荷 */
export interface EncodingProgress {
  sessionId: string;
  generation: number;
  percent: number;
  encoding: string;
  lineCount: number;
  done: boolean;
  failed: boolean;
  error?: string;
}

/** 打开会话的结果 */
export interface OpenSessionResult {
  sessionId: string;
  /** 当前会话实际读取的最外层磁盘源绝对路径。 */
  sourcePath: string;
  /** 条目路径(用于面包屑) */
  entryPath: string;
  /** 解压后大小 */
  size: number;
  /** 是否需要后台解压/建索引(压缩条目 / 大文件) */
  indexing: boolean;
  /** 检测到的编码 */
  encoding: string;
  /** 本次打开因后端会话上限而被 LRU 回收的旧 session。 */
  evictedSessionIds: string[];
}

/** 将当前只读会话缓存导出到用户选择路径后的结果。 */
export interface SnapshotExportResult {
  bytes: number;
  /** false 表示索引仍在进行，仅导出了调用时已经稳定写入的部分。 */
  complete: boolean;
}

/** 后端到达检测事件载荷 */
export interface DetectedItem {
  path: string;
  name: string;
  kind: 'archive' | 'file';
  size: number;
  source: string;
}

/** 后端校验并规范化后的拖入文件信息。 */
export interface DroppedFileInfo {
  path: string;
  name: string;
  kind: 'directory' | 'archive' | 'file';
  watchPath: string;
  isLog: boolean;
  alreadyMonitored: boolean;
}

export interface FileSearchConfig {
  version: number;
  enabled: boolean;
  roots: string[];
  exclusions: string[];
}

export interface FileSearchFeatureState {
  currentEnabled: boolean;
  nextLaunchEnabled: boolean;
}

export type FileSearchPhase = 'disabled' | 'scanning' | 'finalizing' | 'ready' | 'paused' | 'error';

export interface FileSearchProviderStatus {
  root: string;
  provider: 'windowsNtfs' | 'folderScan' | string;
  phase: string;
  stage?: string;
  discoveredRecords?: number;
  searchableFiles?: number;
  startedMs?: number;
  elapsedMs?: number;
  stageStartedMs?: number;
  stageElapsedMs?: number;
  completedMs?: number;
  fallbackReason?: string;
}

export interface FileSearchStatus {
  phase: FileSearchPhase;
  scannedFiles: number;
  skippedDirectories: number;
  indexedFiles: number;
  indexBytes: number;
  roots: string[];
  exclusions: string[];
  providers: FileSearchProviderStatus[];
  error?: string;
}

export type FileSearchServiceErrorCode =
  | 'missing'
  | 'accessDenied'
  | 'startFailed'
  | 'notReady'
  | 'protocolMismatch'
  | 'elevationCancelled'
  | 'repairExecutableMissing'
  | 'repairFailed';

export type FileSearchFilter = 'all' | 'log' | 'archive';

export interface FileSearchResult {
  path: string;
  name: string;
  parent: string;
  kind: 'file' | 'log' | 'archive';
  size: number;
  modifiedMs?: number;
  readable?: boolean;
  contentType?: string;
  isLog: boolean;
  isArchive: boolean;
}

export interface FileSearchPage {
  items: FileSearchResult[];
  total: number;
  partial: boolean;
  elapsedMs: number;
}

/** 最外层磁盘源的存在性与稳定修订标识。 */
export interface FileRevision {
  exists: boolean;
  revision?: string;
}

/** 后端归一化后的单个目录结构变化。 */
export type DirectoryChange =
  | { type: 'upsert'; node: TreeNode }
  | { type: 'remove'; path: string }
  | { type: 'rename'; oldPath: string; node: TreeNode }
  | { type: 'rescan'; nodes: TreeNode[] };

/** 同一监控目录在短时间窗口内合并后的结构变化。 */
export interface DirectoryChangeBatch {
  watchDir: string;
  changes: DirectoryChange[];
}

/** 后缀筛选规则 */
export interface FilterRule {
  /** 勾选启用的后缀 */
  suffixes: string[];
  /** 是否显示全部(含非日志) */
  showAll: boolean;
}

/** updater 返回的新版本元数据 */
export interface AppUpdateInfo {
  currentVersion: string;
  version: string;
  date?: string;
  body?: string;
}

/** 更新包下载与安装阶段进度 */
export interface AppUpdateProgress {
  phase: 'downloading' | 'installing';
  downloadedBytes: number;
  totalBytes?: number;
  /** 总大小未知时不提供百分比；安装阶段固定为 100 */
  percent?: number;
}

export interface AiProviderConfig {
  id: string;
  name: string;
  baseUrl: string;
  model: string;
  keyConfigured: boolean;
  protocol: 'chatCompletions' | 'responses';
  endpointMode: 'base' | 'full';
  allowInsecureHttp: boolean;
}

export interface AiAnalysisResult {
  providerId: string;
  model: string;
  content: string;
}

export interface AiAttachmentSummary {
  path: string;
  name: string;
  charCount: number;
}

export interface AiHistoryMessage {
  role: 'user' | 'assistant';
  content: string;
  attachments?: AiHistoryAttachment[];
}
export interface AiHistoryAttachment {
  name: string;
  charCount: number;
}
export interface AiHistoryRecord {
  id: string;
  title: string;
  createdAt: string;
  updatedAt: string;
  providerId: string;
  protocol: 'chatCompletions' | 'responses';
  model: string;
  endpointFingerprint: string;
  selectedText: string;
  messages: AiHistoryMessage[];
}
export interface AiHistorySummary {
  id: string;
  title: string;
  createdAt: string;
  updatedAt: string;
  providerId: string;
  model: string;
}
