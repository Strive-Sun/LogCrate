import type { CSSProperties } from 'react';

export type LogFontFamily = 'default' | 'consolas' | 'lucida' | 'yahei';
export type LogFontSize = 12 | 13 | 14 | 15 | 16 | 18 | 20;

export interface LogFontPreference {
  family: LogFontFamily;
  size: LogFontSize;
}

export const LOG_FONT_STORAGE_KEY = 'logcrate.logFont.v1';
export const LOG_FONT_FAMILIES: readonly LogFontFamily[] = [
  'default',
  'consolas',
  'lucida',
  'yahei',
];
export const LOG_FONT_SIZES: readonly LogFontSize[] = [12, 13, 14, 15, 16, 18, 20];
export const DEFAULT_LOG_FONT: LogFontPreference = { family: 'default', size: 12 };

// 每个预设都带 macOS 与中文回退;默认等宽与原有 --log-font 保持一致。
const FONT_STACKS: Record<LogFontFamily, string> = {
  default: "'Cascadia Code', 'Consolas', 'SF Mono', 'Menlo', monospace",
  consolas: "'Consolas', 'Menlo', 'Microsoft YaHei', monospace",
  lucida: "'Lucida Console', 'Monaco', 'Microsoft YaHei', monospace",
  yahei: "'Microsoft YaHei UI', 'Microsoft YaHei', 'PingFang SC', 'Segoe UI', sans-serif",
};

export function isLogFontFamily(value: unknown): value is LogFontFamily {
  return LOG_FONT_FAMILIES.includes(value as LogFontFamily);
}

export function isLogFontSize(value: unknown): value is LogFontSize {
  return LOG_FONT_SIZES.includes(value as LogFontSize);
}

/** 字体与字号逐项校验,无效的那一项回退默认值。 */
export function loadLogFont(storage: Pick<Storage, 'getItem'>): LogFontPreference {
  try {
    const raw = storage.getItem(LOG_FONT_STORAGE_KEY);
    if (!raw) return DEFAULT_LOG_FONT;
    const parsed = JSON.parse(raw) as Partial<Record<keyof LogFontPreference, unknown>>;
    return {
      family: isLogFontFamily(parsed?.family) ? parsed.family : DEFAULT_LOG_FONT.family,
      size: isLogFontSize(parsed?.size) ? parsed.size : DEFAULT_LOG_FONT.size,
    };
  } catch {
    return DEFAULT_LOG_FONT;
  }
}

export function saveLogFont(
  storage: Pick<Storage, 'setItem'>,
  preference: LogFontPreference,
): void {
  try {
    storage.setItem(LOG_FONT_STORAGE_KEY, JSON.stringify(preference));
  } catch {
    // WebView 存储不可用时仍保留当前进程内选择。
  }
}

/** 行高约为字号 1.5 倍;12px 时为 18px,与原有固定行高一致。 */
export function logRowHeight(size: LogFontSize): number {
  return Math.round(size * 1.5);
}

/** 行号槽宽随字号缩放;12px 时为 72px,与原有宽度一致。 */
export function logGutterWidth(size: LogFontSize): number {
  return Math.round(size * 6);
}

/** 写到日志面板容器上的 CSS 变量,只影响日志正文、行号与字段筛选栏。 */
export function logFontStyle(preference: LogFontPreference): CSSProperties {
  return {
    '--log-font': FONT_STACKS[preference.family],
    '--log-font-size': `${preference.size}px`,
    '--log-gutter-width': `${logGutterWidth(preference.size)}px`,
  } as CSSProperties;
}
