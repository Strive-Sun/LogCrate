import { useCallback, useEffect, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { api, type DocxPreviewBlock, type OpenDocxSessionResult } from '../api';
import { useI18n } from '../i18n/I18nProvider';

const PAGE = 100;
const MAX_CACHED_BLOCKS = 600;
export const MAX_DOCX_BLOB_BYTES = 64 * 1024 * 1024;

interface BlobEntry {
  url: string;
  bytes: number;
}

export class DocxBlobLru {
  private entries = new Map<string, BlobEntry>();
  private total = 0;

  constructor(private readonly limit = MAX_DOCX_BLOB_BYTES) {}

  get(id: string): string | undefined {
    const entry = this.entries.get(id);
    if (!entry) return undefined;
    this.entries.delete(id);
    this.entries.set(id, entry);
    return entry.url;
  }

  put(id: string, bytes: Uint8Array, mimeType: string): string {
    this.remove(id);
    const url = URL.createObjectURL(new Blob([bytes.slice()], { type: mimeType }));
    this.entries.set(id, { url, bytes: bytes.byteLength });
    this.total += bytes.byteLength;
    while (this.total > this.limit && this.entries.size > 1) {
      const oldest = this.entries.keys().next().value as string | undefined;
      if (!oldest) break;
      this.remove(oldest);
    }
    return url;
  }

  clear(): void {
    for (const entry of this.entries.values()) URL.revokeObjectURL(entry.url);
    this.entries.clear();
    this.total = 0;
  }

  private remove(id: string): void {
    const entry = this.entries.get(id);
    if (!entry) return;
    URL.revokeObjectURL(entry.url);
    this.entries.delete(id);
    this.total -= entry.bytes;
  }
}

function isBoundary(value?: string): boolean {
  return value === undefined || !/[\p{L}\p{N}_]/u.test(value);
}

export function docxTextMatches(
  text: string,
  query: string,
  wholeWord: boolean,
  matchCase: boolean,
) {
  if (!query) return false;
  const source = matchCase ? text : text.toLocaleLowerCase();
  const target = matchCase ? query : query.toLocaleLowerCase();
  let at = source.indexOf(target);
  while (at >= 0) {
    if (!wholeWord || (isBoundary(source[at - 1]) && isBoundary(source[at + target.length])))
      return true;
    at = source.indexOf(target, at + Math.max(1, target.length));
  }
  return false;
}

function DocxImage({
  sessionId,
  block,
  cache,
}: {
  sessionId: string;
  block: Extract<DocxPreviewBlock, { kind: 'image' }>;
  cache: DocxBlobLru;
}) {
  const { t } = useI18n();
  const [url, setUrl] = useState(() => cache.get(block.imageId));
  const [error, setError] = useState<string>();
  useEffect(() => {
    if (block.status !== 'supported' || url) return;
    let active = true;
    void api
      .readDocxImage(sessionId, block.imageId)
      .then((bytes) => {
        if (active)
          setUrl(cache.put(block.imageId, bytes, block.mimeType ?? 'application/octet-stream'));
      })
      .catch((reason) => active && setError(String(reason)));
    return () => {
      active = false;
    };
  }, [block.imageId, block.mimeType, block.status, cache, sessionId, url]);
  if (block.status !== 'supported' || error)
    return (
      <div
        className="docx-image-placeholder"
        role="img"
        aria-label={block.altText ?? t('docx.image')}
      >
        <strong>{block.altText ?? t('docx.imageUnavailable')}</strong>
        <span>{error ?? block.status}</span>
      </div>
    );
  if (!url) return <div className="docx-image-placeholder">{t('docx.imageLoading')}</div>;
  return <img className="docx-image" src={url} alt={block.altText ?? ''} />;
}

export function DocxPreview({
  session,
  active = true,
}: {
  session: OpenDocxSessionResult;
  active?: boolean;
}) {
  const { t } = useI18n();
  const scrollRef = useRef<HTMLDivElement>(null);
  const [cache] = useState(() => new DocxBlobLru());
  const [blocks, setBlocks] = useState(new Map<number, DocxPreviewBlock>());
  const loading = useRef(new Set<number>());
  const [findOpen, setFindOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [reverse, setReverse] = useState(false);
  const [wholeWord, setWholeWord] = useState(false);
  const [matchCase, setMatchCase] = useState(false);
  const [wrap, setWrap] = useState(true);
  const [match, setMatch] = useState<number>();
  const [findStatus, setFindStatus] = useState<string>();

  const loadPage = useCallback(
    async (index: number) => {
      const start = Math.floor(index / PAGE) * PAGE;
      if (loading.current.has(start) || blocks.has(index)) return;
      loading.current.add(start);
      try {
        const page = await api.readDocxBlocks(session.sessionId, start, PAGE);
        setBlocks((current) => {
          const next = new Map(current);
          page.forEach((block) => next.set(block.index, block));
          while (next.size > MAX_CACHED_BLOCKS) {
            const oldest = next.keys().next().value as number | undefined;
            if (oldest === undefined) break;
            next.delete(oldest);
          }
          return next;
        });
      } finally {
        loading.current.delete(start);
      }
    },
    [blocks, session.sessionId],
  );

  const virtualizer = useVirtualizer({
    count: session.blockCount,
    getScrollElement: () => scrollRef.current,
    initialRect: { width: 800, height: 600 },
    estimateSize: (index) => (blocks.get(index)?.kind === 'image' ? 280 : 32),
    overscan: 8,
  });
  const virtualItems = virtualizer.getVirtualItems();
  const displayItems =
    virtualItems.length > 0
      ? virtualItems
      : Array.from({ length: Math.min(20, session.blockCount) }, (_, index) => ({
          key: `initial-${index}`,
          index,
          start: index * 32,
        }));
  useEffect(() => {
    for (const item of displayItems) void loadPage(item.index);
  }, [displayItems, loadPage]);
  useEffect(() => {
    cache.clear();
    setBlocks(new Map());
    setMatch(undefined);
    return () => cache.clear();
  }, [cache, session.sessionId]);
  useEffect(() => {
    if (!active) setFindOpen(false);
    const onKeyDown = (event: KeyboardEvent) => {
      if (!active) return;
      if ((event.ctrlKey || event.metaKey) && event.key.toLocaleLowerCase() === 'f') {
        event.preventDefault();
        setFindOpen(true);
      } else if (event.key === 'Escape' && findOpen) {
        setFindOpen(false);
        scrollRef.current?.focus();
      }
    };
    document.addEventListener('keydown', onKeyDown);
    return () => document.removeEventListener('keydown', onKeyDown);
  }, [active, findOpen]);

  const runFind = useCallback(async () => {
    if (!query || session.blockCount === 0) return;
    const direction = reverse ? -1 : 1;
    let index = match === undefined ? (reverse ? session.blockCount - 1 : 0) : match + direction;
    let scanned = 0;
    while (scanned < session.blockCount) {
      if (index < 0 || index >= session.blockCount) {
        if (!wrap) break;
        index = reverse ? session.blockCount - 1 : 0;
      }
      const start = Math.floor(index / PAGE) * PAGE;
      const page = await api.readDocxBlocks(session.sessionId, start, PAGE);
      const ordered = reverse ? [...page].reverse() : page;
      for (const block of ordered) {
        if (reverse ? block.index > index : block.index < index) continue;
        scanned += 1;
        const text = block.kind === 'text' ? block.text : (block.altText ?? '');
        if (docxTextMatches(text, query, wholeWord, matchCase)) {
          setBlocks((current) => new Map(current).set(block.index, block));
          setMatch(block.index);
          setFindStatus(`${block.index + 1} / ${session.blockCount}`);
          virtualizer.scrollToIndex(block.index, { align: 'center' });
          return;
        }
      }
      index = reverse ? start - 1 : start + PAGE;
    }
    setFindStatus(t('find.notFound'));
  }, [
    match,
    matchCase,
    query,
    reverse,
    session.blockCount,
    session.sessionId,
    t,
    virtualizer,
    wholeWord,
    wrap,
  ]);

  return (
    <section className="docx-preview" aria-label={session.title}>
      <header className="docx-preview-header">
        <span>{t('docx.simplifiedPreview')}</span>
        <span>{t('docx.blockCount', { count: session.blockCount })}</span>
      </header>
      {findOpen && (
        <form
          className="log-find-dialog docx-find"
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
              onClick={() => setFindOpen(false)}
            >
              ×
            </button>
          </div>
          <div className="log-find-query">
            <input
              autoFocus
              value={query}
              aria-label={t('find.keyword')}
              onInput={(event) => {
                setQuery(event.currentTarget.value);
                setMatch(undefined);
              }}
            />
            <button type="submit" disabled={!query}>
              {t('find.action')}
            </button>
          </div>
          <div className="log-find-options">
            <label>
              <input
                type="checkbox"
                checked={reverse}
                onChange={(e) => setReverse(e.target.checked)}
              />
              {t('find.reverse')}
            </label>
            <label>
              <input
                type="checkbox"
                checked={wholeWord}
                onChange={(e) => setWholeWord(e.target.checked)}
              />
              {t('find.wholeWord')}
            </label>
            <label>
              <input
                type="checkbox"
                checked={matchCase}
                onChange={(e) => setMatchCase(e.target.checked)}
              />
              {t('find.caseSensitive')}
            </label>
            <label>
              <input type="checkbox" checked={wrap} onChange={(e) => setWrap(e.target.checked)} />
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
      <div className="docx-scroll" ref={scrollRef} tabIndex={-1}>
        <div
          className="docx-virtual"
          style={{
            height: Math.max(virtualizer.getTotalSize(), Math.min(20, session.blockCount) * 32),
          }}
        >
          {displayItems.map((item) => {
            const block = blocks.get(item.index);
            return (
              <article
                key={item.key}
                ref={virtualizer.measureElement}
                data-index={item.index}
                className={`docx-block${match === item.index ? ' current' : ''}`}
                style={{ transform: `translateY(${item.start}px)` }}
              >
                {!block ? (
                  <span>{t('docx.loading')}</span>
                ) : block.kind === 'text' ? (
                  <div className="docx-text">{block.text || '\u00a0'}</div>
                ) : (
                  <DocxImage sessionId={session.sessionId} block={block} cache={cache} />
                )}
              </article>
            );
          })}
        </div>
      </div>
    </section>
  );
}
