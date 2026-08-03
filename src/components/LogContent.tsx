import { useCallback, useEffect, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { api } from '../api';
import type {
  LogFieldCondition,
  LogFieldLayout,
  LogFieldMarkedLine,
  type LogFieldLayoutPattern,
  LogFieldResultMode,
  LogFieldStatistics,
  LogLine,
  LogSearchMatch,
  OpenSessionResult,
} from '../api';
import { fmtNum, fmtSize } from '../util/format';
import {
  clearSavedLogFieldLayout,
  loadLogFieldLayout,
  persistLogFieldLayout,
  savedLayoutFingerprintMatches,
  type LayoutPersistenceTrigger,
  type StoredLogFieldLayout,
} from '../util/logFieldLayoutStorage';
import { LogFieldFilterBar } from './LogFieldFilterBar';
import { LogRow } from './LogRow';
import { useI18n } from '../i18n/I18nProvider';

interface Props {
  session: OpenSessionResult | null;
  activeKey: string | null;
  status?: 'opening' | 'ready' | 'dormant' | 'error';
  error?: string;
  active?: boolean;
}

const PAGE = 200;
const MAX_CACHED_LINES = 5_000;
const ENCODINGS = ['UTF-8', 'GBK', 'GB18030', 'UTF-16LE', 'UTF-16BE'];

export function mergeLogLineWindow(
  previous: Map<number, LogLine>,
  start: number,
  lines: LogLine[],
  requestGeneration: number,
  currentGeneration: number,
): Map<number, LogLine> {
  if (requestGeneration !== currentGeneration) return previous;
  const next = new Map(previous);
  lines.forEach((line, offset) => next.set(start + offset, line));
  while (next.size > MAX_CACHED_LINES) {
    const oldest = next.keys().next().value;
    if (oldest === undefined) break;
    next.delete(oldest);
  }
  return next;
}

export function storedToRuntime(layout: StoredLogFieldLayout): LogFieldLayout {
  let pattern: LogFieldLayoutPattern = { kind: 'manualColumns' };
  if (layout.source === 'automatic') {
    try {
      const fingerprint = JSON.parse(layout.fingerprint) as {
        pattern?: LogFieldLayoutPattern;
      };
      if (
        fingerprint.pattern?.kind === 'chromium' ||
        fingerprint.pattern?.kind === 'androidLogcat' ||
        fingerprint.pattern?.kind === 'manualColumns' ||
        (fingerprint.pattern?.kind === 'bracketed' &&
          Number.isInteger(fingerprint.pattern.segmentCount) &&
          fingerprint.pattern.segmentCount > 0)
      ) {
        pattern = fingerprint.pattern;
      }
    } catch {
      // Older or malformed fingerprints remain fixed-column layouts.
    }
  }
  return {
    fields: layout.fields.map((field) => ({
      id: field.id,
      name: field.name,
      fieldType: field.type,
      boundary: { start: field.start, end: field.end },
      displayWidth: Math.max(4, (field.end ?? field.start + 24) - field.start),
    })),
    pattern,
    confidence: 1,
    source: layout.source,
  };
}

function layoutFingerprint(layout: LogFieldLayout): string {
  return JSON.stringify({ pattern: layout.pattern, fields: layout.fields.length });
}

function fallbackBodyLayout(name: string): LogFieldLayout {
  return {
    fields: [
      {
        id: 'field-1',
        name,
        fieldType: 'text',
        boundary: { start: 0, end: null },
        displayWidth: 80,
      },
    ],
    pattern: { kind: 'manualColumns' },
    confidence: 0,
    source: 'manual',
  };
}

function runtimeToStored(
  layout: LogFieldLayout,
  encoding: string,
  fingerprint = layoutFingerprint(layout),
): StoredLogFieldLayout {
  return {
    fields: layout.fields.map((field) => ({
      id: field.id,
      name: field.name,
      type: field.fieldType,
      start: field.boundary.start,
      end: field.boundary.end,
    })),
    fingerprint,
    encodingHint: encoding,
    source: layout.source,
  };
}

export function LogContent({ session, activeKey, status = 'ready', error, active = true }: Props) {
  const { t } = useI18n();
  const scrollRef = useRef<HTMLDivElement>(null);
  const [percent, setPercent] = useState(100);
  const [indexedLines, setIndexedLines] = useState(0);
  const [totalLines, setTotalLines] = useState(0);
  const [indexing, setIndexing] = useState(false);
  const [effectiveEncoding, setEffectiveEncoding] = useState('Detecting');
  const [detectedEncoding, setDetectedEncoding] = useState('Detecting');
  const [encodingChanging, setEncodingChanging] = useState(false);
  const [encodingPercent, setEncodingPercent] = useState(0);
  // 行缓存:行号 → 内容
  const [cache, setCache] = useState<Map<number, LogLine>>(new Map());
  const [currentLine, setCurrentLine] = useState(1);
  const pending = useRef<Set<number>>(new Set());
  const encodingUnsub = useRef<() => void>(() => {});
  const preferredEncoding = useRef<string | null>(null);
  const findInputRef = useRef<HTMLInputElement>(null);
  const findGeneration = useRef(0);
  const [findOpen, setFindOpen] = useState(false);
  const [findQuery, setFindQuery] = useState('');
  const [findReverse, setFindReverse] = useState(false);
  const [findWholeWord, setFindWholeWord] = useState(false);
  const [findCaseSensitive, setFindCaseSensitive] = useState(false);
  const [findWrap, setFindWrap] = useState(true);
  const [findBusy, setFindBusy] = useState(false);
  const [findStatus, setFindStatus] = useState<string | null>(null);
  const [findMatch, setFindMatch] = useState<LogSearchMatch | null>(null);
  const [fieldLayout, setFieldLayout] = useState<LogFieldLayout | null>(null);
  const [fieldConditions, setFieldConditions] = useState<LogFieldCondition[]>([]);
  const [fieldStatistics, setFieldStatistics] = useState<LogFieldStatistics[]>([]);
  const [fieldGeneration, setFieldGeneration] = useState<number | null>(null);
  const [fieldScanned, setFieldScanned] = useState(0);
  const [fieldMatched, setFieldMatched] = useState(0);
  const [fieldUnparsed, setFieldUnparsed] = useState(0);
  const [fieldFiltering, setFieldFiltering] = useState(false);
  const [fieldRecognizing, setFieldRecognizing] = useState(false);
  const [fieldError, setFieldError] = useState<string | null>(null);
  const [fieldMode, setFieldMode] = useState<LogFieldResultMode>('compact');
  const [includeUnparsed, setIncludeUnparsed] = useState(true);
  const [fieldFilterMenuOpen, setFieldFilterMenuOpen] = useState(false);
  const [logScrollLeft, setLogScrollLeft] = useState(0);
  const [fieldEncodingVersion, setFieldEncodingVersion] = useState(0);
  const fieldUnsub = useRef<() => void>(() => {});
  const fieldRequestGeneration = useRef(0);
  const fieldAnchor = useRef(1);
  const fieldRestoreAnchor = useRef<number | null>(null);
  const fieldUserInteracted = useRef(false);
  const fieldLayoutFingerprint = useRef<string | null>(null);
  const cacheRequestGeneration = useRef(0);
  const currentLineRef = useRef(1);
  const fieldModeRef = useRef<LogFieldResultMode>('compact');
  const includeUnparsedRef = useRef(true);
  const indexedLinesRef = useRef(0);
  currentLineRef.current = currentLine;
  fieldModeRef.current = fieldMode;
  includeUnparsedRef.current = includeUnparsed;
  indexedLinesRef.current = indexedLines;

  const clearLineCache = useCallback(() => {
    cacheRequestGeneration.current += 1;
    setCache(new Map());
    pending.current = new Set();
  }, []);

  const rebuildForEncoding = useCallback(
    async (entryKey: string, encoding: string) => {
      encodingUnsub.current();
      fieldUnsub.current();
      fieldRequestGeneration.current += 1;
      fieldRestoreAnchor.current = null;
      setFieldConditions([]);
      setFieldGeneration(null);
      setFieldFiltering(false);
      setFieldError(null);
      clearLineCache();
      setTotalLines(indexedLinesRef.current);
      setEncodingChanging(true);
      setEncodingPercent(0);
      try {
        const generation = await api.setSessionEncoding(entryKey, encoding);
        encodingUnsub.current = api.subscribeEncodingProgress(entryKey, generation, (progress) => {
          setEncodingPercent(progress.percent);
          if (!progress.done) return;
          setEncodingChanging(false);
          if (progress.failed) {
            alert(t('error.encodingFailed', { error: progress.error ?? t('common.unknown') }));
            return;
          }
          preferredEncoding.current = progress.encoding;
          setEffectiveEncoding(progress.encoding);
          setTotalLines(progress.lineCount);
          setIndexedLines(progress.lineCount);
          clearLineCache();
          scrollRef.current?.scrollTo({ top: 0 });
          setFieldEncodingVersion((value) => value + 1);
        });
      } catch (error) {
        setEncodingChanging(false);
        alert(t('error.encodingFailed', { error: String(error) }));
      }
    },
    [clearLineCache, t],
  );

  useEffect(
    () => () => {
      encodingUnsub.current();
      fieldUnsub.current();
    },
    [],
  );

  // 打开新条目:重置并按需订阅建索引进度
  useEffect(() => {
    if (!session || !activeKey) {
      clearLineCache();
      setTotalLines(0);
      setIndexedLines(0);
      setIndexing(false);
      encodingUnsub.current();
      return;
    }
    const encodingToRestore = preferredEncoding.current;
    if (!encodingToRestore) preferredEncoding.current = session.encoding;
    clearLineCache();
    const total = api.lineCount(activeKey);
    setTotalLines(total);
    setIndexedLines(total);
    setEffectiveEncoding(session.encoding);
    setDetectedEncoding(session.encoding);
    setEncodingChanging(false);
    encodingUnsub.current();
    scrollRef.current?.scrollTo({ top: 0 });

    if (session.indexing) {
      setIndexing(true);
      setPercent(0);
      const unsub = api.subscribeIndexProgress(
        activeKey,
        (p) => {
          setPercent(p.percent);
          setIndexedLines(p.indexedLines);
          setTotalLines(p.indexedLines);
          setDetectedEncoding(p.detectedEncoding);
          setEffectiveEncoding(p.effectiveEncoding);
        },
        (finalTotal) => {
          setIndexing(false);
          setPercent(100);
          setIndexedLines(finalTotal);
          setTotalLines(finalTotal);
          if (encodingToRestore && encodingToRestore !== session.encoding) {
            void rebuildForEncoding(activeKey, encodingToRestore);
          }
        },
      );
      return unsub;
    } else {
      setIndexing(false);
      setPercent(100);
      setIndexedLines(total);
      if (encodingToRestore && encodingToRestore !== session.encoding) {
        void rebuildForEncoding(activeKey, encodingToRestore);
      }
    }
  }, [session, activeKey, rebuildForEncoding, clearLineCache]);

  const applyFieldFilter = useCallback(
    async (entryKey: string, layout: LogFieldLayout, conditions: LogFieldCondition[]) => {
      const requestGeneration = ++fieldRequestGeneration.current;
      fieldUnsub.current();
      fieldAnchor.current = Math.max(1, currentLineRef.current);
      setFieldFiltering(true);
      setFieldError(null);
      clearLineCache();
      try {
        const generation = await api.setLogFieldFilter(entryKey, { layout, conditions });
        if (requestGeneration !== fieldRequestGeneration.current) return;
        // Clearing at request start prevents the previous view from lingering, but that render can
        // still enqueue another page read against the previous field generation. Advance the cache
        // namespace again when the backend activates the new generation so that response cannot
        // populate the new view.
        clearLineCache();
        setFieldGeneration(generation);
        fieldUnsub.current = api.subscribeLogFieldProgress(entryKey, generation, (progress) => {
          if (requestGeneration !== fieldRequestGeneration.current) return;
          setFieldScanned(progress.scannedLines);
          setFieldMatched(progress.matchedLines);
          setFieldUnparsed(progress.unparsedLines);
          setTotalLines(
            fieldModeRef.current === 'highlight'
              ? Math.max(indexedLinesRef.current, progress.totalLines)
              : progress.matchedLines + (includeUnparsedRef.current ? progress.unparsedLines : 0),
          );
          if (!progress.done) return;
          setFieldFiltering(false);
          if (progress.failed) {
            setFieldError(progress.error ?? t('common.unknown'));
            setFieldGeneration(null);
            clearLineCache();
            setTotalLines(indexedLinesRef.current);
            return;
          }
          void api.logFieldStatus(entryKey).then((status) => {
            if (status?.generation === generation) setFieldStatistics(status.statistics);
          });
          void api
            .locateLogFieldAnchor(
              entryKey,
              generation,
              fieldAnchor.current,
              fieldModeRef.current,
              includeUnparsedRef.current,
            )
            .then((anchor) => {
              if (anchor && requestGeneration === fieldRequestGeneration.current) {
                scrollRef.current?.scrollTo({ top: anchor.viewIndex * 18 });
              }
            });
        });
      } catch (error) {
        if (requestGeneration !== fieldRequestGeneration.current) return;
        setFieldFiltering(false);
        setFieldError(String(error));
        setFieldGeneration(null);
        clearLineCache();
        setTotalLines(indexedLinesRef.current);
      }
    },
    [clearLineCache, t],
  );

  useEffect(() => {
    fieldUnsub.current();
    fieldRequestGeneration.current += 1;
    fieldUserInteracted.current = false;
    fieldRestoreAnchor.current = null;
    fieldLayoutFingerprint.current = null;
    setFieldConditions([]);
    setFieldStatistics([]);
    setFieldGeneration(null);
    setFieldError(null);
    setFieldMode('compact');
    setIncludeUnparsed(true);
    if (!session || !activeKey || status !== 'ready') {
      setFieldLayout(null);
      setFieldRecognizing(false);
      return;
    }
    let cancelled = false;
    const saved = loadLogFieldLayout(localStorage, activeKey);
    const start = async () => {
      setFieldRecognizing(true);
      let layout = saved ? storedToRuntime(saved) : null;
      if (layout) {
        fieldLayoutFingerprint.current = saved!.fingerprint;
        setFieldLayout(layout);
        setFieldRecognizing(false);
        await applyFieldFilter(activeKey, layout, []);
        const quick = await api.analyzeLogFieldLayout(activeKey, 'quick');
        if (cancelled) return;
        const matches =
          quick.layout && savedLayoutFingerprintMatches(saved!, layoutFingerprint(quick.layout));
        if (!matches && !window.confirm(t('fields.savedMismatch'))) {
          layout = quick.layout ?? fallbackBodyLayout(t('fields.body'));
          setFieldLayout(layout);
          if (quick.layout) {
            fieldLayoutFingerprint.current = layoutFingerprint(layout);
            persistLogFieldLayout(
              localStorage,
              activeKey,
              runtimeToStored(layout, session.encoding),
              'stableAutomatic',
            );
          } else {
            fieldLayoutFingerprint.current = null;
          }
          await applyFieldFilter(activeKey, layout, []);
        }
      } else {
        const quick = await api.analyzeLogFieldLayout(activeKey, 'quick');
        if (cancelled) return;
        layout = quick.layout ?? fallbackBodyLayout(t('fields.body'));
        setFieldLayout(layout);
        setFieldRecognizing(false);
        if (quick.layout) {
          fieldLayoutFingerprint.current = layoutFingerprint(layout);
          persistLogFieldLayout(
            localStorage,
            activeKey,
            runtimeToStored(layout, session.encoding),
            'stableAutomatic',
          );
        }
        await applyFieldFilter(activeKey, layout, []);
      }
      const background = await api.analyzeLogFieldLayout(activeKey, 'background');
      if (
        cancelled ||
        fieldUserInteracted.current ||
        !background.layout ||
        (layout && background.layout.confidence <= layout.confidence)
      ) {
        return;
      }
      setFieldLayout(background.layout);
      fieldLayoutFingerprint.current = layoutFingerprint(background.layout);
      persistLogFieldLayout(
        localStorage,
        activeKey,
        runtimeToStored(background.layout, session.encoding),
        'stableAutomatic',
      );
      await applyFieldFilter(activeKey, background.layout, []);
    };
    void start().catch((error) => {
      if (!cancelled) {
        setFieldRecognizing(false);
        setFieldError(String(error));
      }
    });
    return () => {
      cancelled = true;
      fieldUnsub.current();
    };
  }, [activeKey, applyFieldFilter, clearLineCache, fieldEncodingVersion, session, status, t]);

  const rowVirtualizer = useVirtualizer({
    count: totalLines,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 18,
    overscan: 20,
  });

  useEffect(() => {
    if (!active || !session || !activeKey) {
      setFindOpen(false);
      findGeneration.current += 1;
      setFindBusy(false);
      setFindMatch(null);
      setFindStatus(null);
      setFindQuery('');
      setFindReverse(false);
      setFindWholeWord(false);
      setFindCaseSensitive(false);
      setFindWrap(true);
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLocaleLowerCase() === 'f') {
        event.preventDefault();
        setFindOpen(true);
        findInputRef.current?.focus();
        return;
      }
      if (event.key === 'Escape' && findOpen) {
        event.preventDefault();
        setFindOpen(false);
        scrollRef.current?.focus();
      }
    };
    document.addEventListener('keydown', onKeyDown);
    return () => document.removeEventListener('keydown', onKeyDown);
  }, [active, activeKey, findOpen, session]);

  useEffect(() => {
    if (findOpen) findInputRef.current?.focus();
  }, [findOpen]);

  useEffect(() => {
    setFindMatch(null);
    setFindStatus(null);
  }, [activeKey, findCaseSensitive, findQuery, findReverse, findWholeWord, findWrap]);

  const runFind = useCallback(async () => {
    const query = findQuery;
    if (!activeKey || !query) return;
    const generation = ++findGeneration.current;
    setFindBusy(true);
    setFindStatus(null);
    const startLine = findMatch ? findMatch.lineNo - 1 : Math.max(0, currentLine - 1);
    const startColumn = findMatch
      ? findReverse
        ? findMatch.startColumn
        : findMatch.endColumn
      : undefined;
    try {
      const result = await api.searchLog(activeKey, {
        query,
        startLine,
        startColumn,
        reverse: findReverse,
        wholeWord: findWholeWord,
        caseSensitive: findCaseSensitive,
        wrap: findWrap,
        ...(fieldGeneration
          ? { fieldView: { generation: fieldGeneration, mode: fieldMode, includeUnparsed } }
          : {}),
      });
      if (generation !== findGeneration.current) return;
      if (result.match) {
        setFindMatch(result.match);
        if (fieldGeneration && fieldMode === 'compact') {
          void api
            .locateLogFieldAnchor(
              activeKey,
              fieldGeneration,
              result.match.lineNo,
              fieldMode,
              includeUnparsed,
            )
            .then((anchor) => {
              if (anchor) rowVirtualizer.scrollToIndex(anchor.viewIndex, { align: 'center' });
            });
        } else {
          rowVirtualizer.scrollToIndex(result.match.lineNo - 1, { align: 'center' });
        }
        setFindStatus(
          result.wrapped ? t(findReverse ? 'find.wrappedEnd' : 'find.wrappedStart') : null,
        );
      } else {
        setFindStatus(
          result.indexing
            ? t('find.notFoundIndexed')
            : !findWrap && result.reachedBoundary
              ? t(findReverse ? 'find.reachedStart' : 'find.reachedEnd')
              : t('find.notFound'),
        );
      }
    } catch (error) {
      if (generation === findGeneration.current) {
        setFindStatus(t('find.failed', { error: String(error) }));
      }
    } finally {
      if (generation === findGeneration.current) setFindBusy(false);
    }
  }, [
    activeKey,
    currentLine,
    findCaseSensitive,
    findMatch,
    findQuery,
    findReverse,
    findWholeWord,
    findWrap,
    fieldGeneration,
    fieldMode,
    includeUnparsed,
    rowVirtualizer,
    t,
  ]);

  // 按可视区批量拉取未缓存的行(窗口化加载)
  const items = rowVirtualizer.getVirtualItems();
  useEffect(() => {
    if (!activeKey || items.length === 0) return;
    const first = items[0].index;
    setCurrentLine(cache.get(first)?.lineNo ?? first + 1);
    const start = Math.floor(first / PAGE) * PAGE;
    const last = items[items.length - 1].index;
    const endPage = Math.floor(last / PAGE) * PAGE;
    for (let p = start; p <= endPage; p += PAGE) {
      const pageLast = Math.min(p + PAGE - 1, totalLines - 1);
      if (pending.current.has(p) || cache.has(pageLast)) continue;
      const pendingPages = pending.current;
      pendingPages.add(p);
      const read = fieldGeneration
        ? fieldMode === 'compact'
          ? api.readFilteredLines(activeKey, fieldGeneration, p, PAGE, includeUnparsed)
          : api.readLinesWithFieldMatches(activeKey, fieldGeneration, p, PAGE)
        : api.readLines(activeKey, p, PAGE);
      const cacheGeneration = cacheRequestGeneration.current;
      read
        .then((lines) => {
          if (cacheGeneration !== cacheRequestGeneration.current) return;
          setCache((prev) =>
            mergeLogLineWindow(prev, p, lines, cacheGeneration, cacheRequestGeneration.current),
          );
        })
        .finally(() => pendingPages.delete(p));
    }
  }, [items, activeKey, cache, totalLines, fieldGeneration, fieldMode, includeUnparsed]);

  function changeFieldConditions(conditions: LogFieldCondition[]) {
    if (!activeKey || !fieldLayout) return;
    fieldUserInteracted.current = true;
    fieldAnchor.current = currentLine;
    if (fieldConditions.length === 0 && conditions.length > 0) {
      fieldRestoreAnchor.current = currentLine;
    }
    setFieldConditions(conditions);
    void applyFieldFilter(activeKey, fieldLayout, conditions);
  }

  function changeFieldLayout(
    layout: LogFieldLayout,
    trigger: 'boundary' | 'name' | 'type' | 'split' | 'merge',
  ) {
    if (!activeKey) return;
    fieldUserInteracted.current = true;
    setFieldLayout(layout);
    setFieldConditions([]);
    const persistenceTrigger: LayoutPersistenceTrigger =
      trigger === 'boundary'
        ? 'boundaryDragCommitted'
        : trigger === 'name'
          ? 'nameCommitted'
          : trigger === 'type'
            ? 'typeChanged'
            : trigger === 'split'
              ? 'fieldSplit'
              : 'fieldMerged';
    persistLogFieldLayout(
      localStorage,
      activeKey,
      runtimeToStored(
        layout,
        effectiveEncoding,
        fieldLayoutFingerprint.current ?? layoutFingerprint(layout),
      ),
      persistenceTrigger,
    );
    void applyFieldFilter(activeKey, layout, []);
  }

  function switchFieldMode(mode: LogFieldResultMode) {
    setFieldMode(mode);
    fieldModeRef.current = mode;
    clearLineCache();
    setTotalLines(
      mode === 'highlight' ? indexedLines : fieldMatched + (includeUnparsed ? fieldUnparsed : 0),
    );
    if (activeKey && fieldGeneration) {
      void api
        .locateLogFieldAnchor(activeKey, fieldGeneration, currentLine, mode, includeUnparsed)
        .then((anchor) => {
          if (anchor) scrollRef.current?.scrollTo({ top: anchor.viewIndex * 18 });
        });
    }
  }

  function toggleUnparsed(show: boolean) {
    setIncludeUnparsed(show);
    includeUnparsedRef.current = show;
    clearLineCache();
    if (fieldMode === 'compact') setTotalLines(fieldMatched + (show ? fieldUnparsed : 0));
  }

  async function clearFieldFilters() {
    if (!activeKey) return;
    fieldUnsub.current();
    fieldRequestGeneration.current += 1;
    await api.clearLogFieldFilter(activeKey);
    setFieldConditions([]);
    setFieldGeneration(null);
    setFieldFiltering(false);
    setFieldError(null);
    clearLineCache();
    setTotalLines(indexedLines);
    const restoreLine = fieldRestoreAnchor.current ?? fieldAnchor.current;
    fieldRestoreAnchor.current = null;
    scrollRef.current?.scrollTo({ top: Math.max(0, restoreLine - 1) * 18 });
  }

  async function reanalyzeFieldLayout() {
    if (!activeKey) return;
    fieldUserInteracted.current = false;
    setFieldRecognizing(true);
    try {
      const analysis = await api.analyzeLogFieldLayout(activeKey, 'quick');
      const layout = analysis.layout ?? fallbackBodyLayout(t('fields.body'));
      setFieldLayout(layout);
      setFieldConditions([]);
      fieldRestoreAnchor.current = null;
      if (analysis.layout) {
        fieldLayoutFingerprint.current = layoutFingerprint(layout);
        persistLogFieldLayout(
          localStorage,
          activeKey,
          runtimeToStored(layout, effectiveEncoding),
          'stableAutomatic',
        );
      } else {
        fieldLayoutFingerprint.current = null;
      }
      await applyFieldFilter(activeKey, layout, []);
      const background = await api.analyzeLogFieldLayout(activeKey, 'background');
      if (
        fieldUserInteracted.current ||
        !background.layout ||
        background.layout.confidence <= layout.confidence
      ) {
        return;
      }
      fieldLayoutFingerprint.current = layoutFingerprint(background.layout);
      setFieldLayout(background.layout);
      persistLogFieldLayout(
        localStorage,
        activeKey,
        runtimeToStored(background.layout, effectiveEncoding),
        'stableAutomatic',
      );
      await applyFieldFilter(activeKey, background.layout, []);
    } catch (error) {
      setFieldError(String(error));
    } finally {
      setFieldRecognizing(false);
    }
  }

  async function changeEncoding(encoding: string) {
    if (!activeKey || encoding === effectiveEncoding) return;
    await rebuildForEncoding(activeKey, encoding);
  }

  if (!session && !activeKey) {
    return (
      <div className="col log-content-panel">
        <div className="empty-state">
          <div className="big">📄</div>
          <div className="desc">{t('log.choose')}</div>
        </div>
      </div>
    );
  }

  if (!session && activeKey) {
    const message =
      status === 'error'
        ? t('log.openFailed', { error: error ?? t('common.unknown') })
        : status === 'dormant'
          ? t('log.dormant')
          : t('log.opening');
    return (
      <div className="col log-content-panel">
        <div className="empty-state log-session-state">
          {status === 'error' && <div className="big">⚠</div>}
          <div className="desc">{message}</div>
        </div>
      </div>
    );
  }

  return (
    <div className="col log-content-panel">
      {indexing && (
        <div className="index-bar">
          <span>
            {t(activeKey?.includes('::') ? 'log.extractingAndIndexing' : 'log.indexing', {
              percent,
            })}
          </span>
          <div className="track">
            <div className="fill" style={{ width: `${percent}%` }} />
          </div>
          <span>{t('log.readable', { count: fmtNum(indexedLines) })}</span>
        </div>
      )}

      {encodingChanging && (
        <div className="index-bar">
          <span>{t('log.reencoding', { percent: encodingPercent })}</span>
          <div className="track">
            <div className="fill" style={{ width: `${encodingPercent}%` }} />
          </div>
          <span>{effectiveEncoding}</span>
        </div>
      )}

      <LogFieldFilterBar
        layout={fieldLayout}
        conditions={fieldConditions}
        statistics={fieldStatistics}
        scrollLeft={logScrollLeft}
        recognizing={fieldRecognizing}
        onConditionsChange={changeFieldConditions}
        onLayoutChange={changeFieldLayout}
      />

      {findOpen && (
        <form
          className="log-find-dialog"
          role="dialog"
          aria-label={t('find.title')}
          onSubmit={(event) => {
            event.preventDefault();
            void runFind();
          }}
        >
          <div className="log-find-head">
            <strong>{t('find.title')}</strong>
            <button
              type="button"
              className="log-find-close"
              aria-label={t('find.close')}
              onClick={() => {
                setFindOpen(false);
                scrollRef.current?.focus();
              }}
            >
              ×
            </button>
          </div>
          <div className="log-find-query">
            <input
              ref={findInputRef}
              value={findQuery}
              aria-label={t('find.keyword')}
              placeholder={t('find.placeholder')}
              onInput={(event) => setFindQuery(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key !== 'Enter') return;
                event.preventDefault();
                void runFind();
              }}
            />
            <button type="submit" disabled={!findQuery || findBusy}>
              {findBusy ? t('find.searching') : t('find.action')}
            </button>
          </div>
          <div className="log-find-options">
            <label>
              <input
                type="checkbox"
                checked={findReverse}
                onChange={(event) => setFindReverse(event.target.checked)}
              />
              {t('find.reverse')}
            </label>
            <label>
              <input
                type="checkbox"
                checked={findWholeWord}
                onChange={(event) => setFindWholeWord(event.target.checked)}
              />
              {t('find.wholeWord')}
            </label>
            <label>
              <input
                type="checkbox"
                checked={findCaseSensitive}
                onChange={(event) => setFindCaseSensitive(event.target.checked)}
              />
              {t('find.caseSensitive')}
            </label>
            <label>
              <input
                type="checkbox"
                checked={findWrap}
                onChange={(event) => setFindWrap(event.target.checked)}
              />
              {t('find.wrap')}
            </label>
          </div>
          {findStatus && (
            <div className="log-find-status" role="status">
              {findStatus}
            </div>
          )}
        </form>
      )}

      <div
        className="log-view"
        ref={scrollRef}
        tabIndex={-1}
        onScroll={(event) => setLogScrollLeft(event.currentTarget.scrollLeft)}
      >
        {fieldGeneration && !fieldFiltering && fieldMode === 'compact' && totalLines === 0 && (
          <div className="log-field-empty">
            <span>{t('fields.noResults')}</span>
            <button type="button" onClick={() => void clearFieldFilters()}>
              {t('fields.clear')}
            </button>
          </div>
        )}
        <div
          style={{
            height: rowVirtualizer.getTotalSize(),
            position: 'relative',
            minWidth: 'max-content',
          }}
        >
          {items.map((vi) => {
            const line = cache.get(vi.index);
            const ready = vi.index < indexedLines || !indexing;
            return (
              <LogRow
                key={vi.index}
                top={vi.start}
                lineNo={line?.lineNo ?? vi.index + 1}
                line={line}
                ready={fieldGeneration ? true : ready}
                fieldMatched={
                  fieldMode === 'highlight' &&
                  Boolean((line as LogFieldMarkedLine | undefined)?.fieldMatched)
                }
                match={findMatch}
                findQuery={findQuery}
                findWholeWord={findWholeWord}
                findCaseSensitive={findCaseSensitive}
                showAllFindMatches={findOpen}
              />
            );
          })}
        </div>
      </div>

      <div className="col-foot log-status-foot">
        <select
          className="encoding-select"
          value={effectiveEncoding}
          disabled={indexing || encodingChanging}
          title={t('log.autoDetected', { encoding: detectedEncoding })}
          onChange={(event) => void changeEncoding(event.target.value)}
        >
          {effectiveEncoding === 'Detecting' && (
            <option value="Detecting">{t('log.detecting')}</option>
          )}
          {ENCODINGS.map((encoding) => (
            <option key={encoding} value={encoding}>
              {encoding}
            </option>
          ))}
        </select>
        <div className={`log-filter-menu${fieldFilterMenuOpen ? ' open' : ''}`}>
          <button
            type="button"
            className="log-filter-menu-summary"
            aria-expanded={fieldFilterMenuOpen}
            onClick={() => setFieldFilterMenuOpen((open) => !open)}
          >
            {t('fields.filterMenu')}
            {fieldConditions.length > 0 ? ` (${fieldConditions.length})` : ''} ▾
          </button>
          <div className="log-filter-menu-content">
            <select
              className="log-field-mode-select"
              aria-label={t('fields.resultMode')}
              value={fieldMode}
              disabled={!fieldLayout}
              onChange={(event) => switchFieldMode(event.target.value as LogFieldResultMode)}
            >
              <option value="compact">{t('fields.compact')}</option>
              <option value="highlight">{t('fields.highlight')}</option>
            </select>
            <label
              className="log-field-unparsed-toggle"
              title={fieldMode === 'highlight' ? t('fields.unparsedHighlightHint') : undefined}
            >
              <input
                type="checkbox"
                checked={includeUnparsed}
                disabled={!fieldLayout || fieldMode === 'highlight'}
                onChange={(event) => toggleUnparsed(event.target.checked)}
              />
              {t('fields.unparsed')}
            </label>
            <button
              type="button"
              className="log-field-foot-button"
              disabled={fieldConditions.length === 0 && !fieldGeneration}
              onClick={() => void clearFieldFilters()}
            >
              {t('fields.clear')}
            </button>
            <details className="log-layout-menu">
              <summary>{t('fields.layout')} ▾</summary>
              <div>
                <button type="button" onClick={() => void reanalyzeFieldLayout()}>
                  {t('fields.reanalyze')}
                </button>
                <button
                  type="button"
                  onClick={() => {
                    if (activeKey) clearSavedLogFieldLayout(localStorage, activeKey);
                  }}
                >
                  {t('fields.clearSaved')}
                </button>
                <small>{t('fields.autoSaveHint')}</small>
              </div>
            </details>
          </div>
        </div>
        <span className="log-field-status-text">
          {fieldError
            ? t('fields.failed', { error: fieldError })
            : fieldFiltering
              ? t(
                  fieldMode === 'compact' ? 'fields.filteringCompact' : 'fields.filteringHighlight',
                  {
                    matched: fmtNum(fieldMatched),
                    scanned: fmtNum(fieldScanned),
                    total: fmtNum(indexedLines),
                  },
                )
              : fieldGeneration
                ? t('fields.complete', {
                    matched: fmtNum(fieldMatched),
                    unparsed: fmtNum(fieldUnparsed),
                  })
                : t('log.lineStatus', { current: fmtNum(currentLine), total: fmtNum(totalLines) })}
        </span>
        <span className="log-file-size">{fmtSize(session?.size)}</span>
      </div>
    </div>
  );
}
