import { memo } from 'react';
import type { LogLine, LogSearchMatch } from '../api';
import { detectLevel } from '../util/format';
import { useI18n } from '../i18n/I18nProvider';

interface Props {
  top: number;
  lineNo: number;
  line: LogLine | undefined;
  ready: boolean;
  match?: LogSearchMatch | null;
}

/** 单行日志:行号固定在左、内容可横向滚动、级别着色、截断标记 */
export const LogRow = memo(function LogRow({ top, lineNo, line, ready, match }: Props) {
  const { t } = useI18n();
  const lvl = line ? detectLevel(line.content) : null;
  const highlighted = line && match?.lineNo === lineNo;
  return (
    <div className="log-line" style={{ position: 'absolute', top, left: 0, right: 0, height: 18 }}>
      <span className="ln">{lineNo}</span>
      <span className="txt">
        {line ? (
          <span className={lvl ? `lvl-${lvl}` : undefined}>
            {highlighted ? (
              <>
                {line.content.slice(0, match.startColumn)}
                <mark className="log-find-match">
                  {line.content.slice(match.startColumn, match.endColumn)}
                </mark>
                {line.content.slice(match.endColumn)}
              </>
            ) : (
              line.content
            )}
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
