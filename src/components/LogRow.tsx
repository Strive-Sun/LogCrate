import { memo } from 'react';
import type { LogLine, LogSearchMatch } from '../api';
import { detectLevel } from '../util/format';
import { findKeywordMatches } from '../util/logSearch';
import { useI18n } from '../i18n/I18nProvider';

interface Props {
  top: number;
  lineNo: number;
  line: LogLine | undefined;
  ready: boolean;
  match?: LogSearchMatch | null;
  findQuery?: string;
  findWholeWord?: boolean;
  findCaseSensitive?: boolean;
  showAllFindMatches?: boolean;
}

/** 单行日志:行号固定在左、内容可横向滚动、级别着色、截断标记 */
export const LogRow = memo(function LogRow({
  top,
  lineNo,
  line,
  ready,
  match,
  findQuery = '',
  findWholeWord = false,
  findCaseSensitive = false,
  showAllFindMatches = false,
}: Props) {
  const { t } = useI18n();
  const lvl = line ? detectLevel(line.content) : null;
  const currentMatch =
    line &&
    match?.lineNo === lineNo &&
    match.startColumn >= 0 &&
    match.endColumn > match.startColumn &&
    match.endColumn <= line.content.length
      ? { startColumn: match.startColumn, endColumn: match.endColumn }
      : null;
  const matches =
    line && showAllFindMatches && findQuery
      ? findKeywordMatches(line.content, findQuery, {
          wholeWord: findWholeWord,
          caseSensitive: findCaseSensitive,
        })
      : currentMatch
        ? [currentMatch]
        : [];
  const content = line
    ? matches.map((range, index) => {
        const previousEnd = index === 0 ? 0 : matches[index - 1].endColumn;
        const isCurrent =
          currentMatch?.startColumn === range.startColumn &&
          currentMatch.endColumn === range.endColumn;
        return (
          <span key={range.startColumn}>
            {line.content.slice(previousEnd, range.startColumn)}
            <mark
              className={isCurrent ? 'log-find-match log-find-match-current' : 'log-find-match'}
            >
              {line.content.slice(range.startColumn, range.endColumn)}
            </mark>
            {index === matches.length - 1 ? line.content.slice(range.endColumn) : null}
          </span>
        );
      })
    : null;
  return (
    <div className="log-line" style={{ position: 'absolute', top, left: 0, right: 0, height: 18 }}>
      <span className="ln">{lineNo}</span>
      <span className="txt">
        {line ? (
          <span className={lvl ? `lvl-${lvl}` : undefined}>
            {matches.length > 0 ? content : line.content}
          </span>
        ) : ready ? (
          <span style={{ color: 'var(--fg-faint)' }}>{t('log.loading')}</span>
        ) : (
          <span style={{ color: 'var(--fg-faint)' }}>{t('log.extracting')}</span>
        )}
        {line?.truncated && <span className="trunc-tag">{t('log.truncated')}</span>}
      </span>
    </div>
  );
});
