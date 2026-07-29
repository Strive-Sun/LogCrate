import { useCallback, useEffect, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { api } from '../api';
import type { LogLine, LogSearchMatch, OpenSessionResult } from '../api';
import { fmtNum, fmtSize } from '../util/format';
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

  const rebuildForEncoding = useCallback(
    async (entryKey: string, encoding: string) => {
      encodingUnsub.current();
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
          setCache(new Map());
          pending.current = new Set();
          scrollRef.current?.scrollTo({ top: 0 });
        });
      } catch (error) {
        setEncodingChanging(false);
        alert(t('error.encodingFailed', { error: String(error) }));
      }
    },
    [t],
  );

  useEffect(
    () => () => {
      encodingUnsub.current();
    },
    [],
  );

  // 打开新条目:重置并按需订阅建索引进度
  useEffect(() => {
    if (!session || !activeKey) {
      setCache(new Map());
      pending.current = new Set();
      setTotalLines(0);
      setIndexedLines(0);
      setIndexing(false);
      encodingUnsub.current();
      return;
    }
    const encodingToRestore = preferredEncoding.current;
    if (!encodingToRestore) preferredEncoding.current = session.encoding;
    setCache(new Map());
    pending.current = new Set();
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
  }, [session, activeKey, rebuildForEncoding]);

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
      });
      if (generation !== findGeneration.current) return;
      if (result.match) {
        setFindMatch(result.match);
        rowVirtualizer.scrollToIndex(result.match.lineNo - 1, { align: 'center' });
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
    rowVirtualizer,
    t,
  ]);

  // 按可视区批量拉取未缓存的行(窗口化加载)
  const items = rowVirtualizer.getVirtualItems();
  useEffect(() => {
    if (!activeKey || items.length === 0) return;
    const first = items[0].index;
    setCurrentLine(first + 1);
    const start = Math.floor(first / PAGE) * PAGE;
    const last = items[items.length - 1].index;
    const endPage = Math.floor(last / PAGE) * PAGE;
    for (let p = start; p <= endPage; p += PAGE) {
      const pageLast = Math.min(p + PAGE - 1, totalLines - 1);
      if (pending.current.has(p) || cache.has(pageLast)) continue;
      pending.current.add(p);
      api
        .readLines(activeKey, p, PAGE)
        .then((lines) => {
          setCache((prev) => {
            const next = new Map(prev);
            for (const l of lines) next.set(l.lineNo - 1, l);
            while (next.size > MAX_CACHED_LINES) {
              const oldest = next.keys().next().value;
              if (oldest === undefined) break;
              next.delete(oldest);
            }
            return next;
          });
        })
        .finally(() => pending.current.delete(p));
    }
  }, [items, activeKey, cache, totalLines]);

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

      <div className="log-view" ref={scrollRef} tabIndex={-1}>
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
                lineNo={vi.index + 1}
                line={line}
                ready={ready}
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

      <div className="col-foot" style={{ display: 'flex', gap: 16 }}>
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
        <span>
          {t('log.lineStatus', { current: fmtNum(currentLine), total: fmtNum(totalLines) })}
        </span>
        <span style={{ marginLeft: 'auto' }}>{fmtSize(session?.size)}</span>
      </div>
    </div>
  );
}
