import type { CSSProperties } from 'react';

export type LogColorKey = 'body' | 'error' | 'warn' | 'info' | 'debug';
/** 缺省的键表示跟随当前主题颜色。 */
export type LogColorPreference = Partial<Record<LogColorKey, string>>;

export const LOG_COLOR_STORAGE_KEY = 'logcrate.logColors.v1';
export const LOG_COLOR_KEYS: readonly LogColorKey[] = ['body', 'error', 'warn', 'info', 'debug'];

// 跟随主题时对应的主题变量;DEBUG 与 TRACE 共用 --debug。
const THEME_VARIABLES: Record<LogColorKey, string> = {
  body: '--fg',
  error: '--error',
  warn: '--warn',
  info: '--info',
  debug: '--debug',
};

export function isHexColor(value: unknown): value is string {
  return typeof value === 'string' && /^#[0-9a-f]{6}$/i.test(value);
}

/** 把 `#rgb`、`#rrggbb` 或 `rgb(r, g, b)` 规范为取色器使用的小写 `#rrggbb`。 */
export function normalizeHexColor(value: string): string | null {
  const text = value.trim().toLowerCase();
  if (/^#[0-9a-f]{6}$/.test(text)) return text;
  const short = /^#([0-9a-f])([0-9a-f])([0-9a-f])$/.exec(text);
  if (short) return `#${short[1]}${short[1]}${short[2]}${short[2]}${short[3]}${short[3]}`;
  const rgb = /^rgba?\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})/.exec(text);
  if (rgb) {
    const channels = rgb.slice(1, 4).map(Number);
    if (channels.some((channel) => channel > 255)) return null;
    return `#${channels.map((channel) => channel.toString(16).padStart(2, '0')).join('')}`;
  }
  return null;
}

/** 逐项校验,非法或缺失的项跟随主题。 */
export function loadLogColors(storage: Pick<Storage, 'getItem'>): LogColorPreference {
  try {
    const raw = storage.getItem(LOG_COLOR_STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as Record<string, unknown> | null;
    if (!parsed || typeof parsed !== 'object') return {};
    const colors: LogColorPreference = {};
    for (const key of LOG_COLOR_KEYS) {
      const value = parsed[key];
      if (isHexColor(value)) colors[key] = value.toLowerCase();
    }
    return colors;
  } catch {
    return {};
  }
}

export function saveLogColors(storage: Pick<Storage, 'setItem'>, colors: LogColorPreference): void {
  try {
    storage.setItem(LOG_COLOR_STORAGE_KEY, JSON.stringify(colors));
  } catch {
    // WebView 存储不可用时仍保留当前进程内选择。
  }
}

/** 设置或清除(`null` 即恢复跟随主题)单项颜色。 */
export function withLogColor(
  colors: LogColorPreference,
  key: LogColorKey,
  color: string | null,
): LogColorPreference {
  const next = { ...colors };
  const normalized = color === null ? null : normalizeHexColor(color);
  if (normalized) next[key] = normalized;
  else delete next[key];
  return next;
}

/** 只为自定义了的项写变量;样式表中未设置的变量回退到主题颜色。 */
export function logColorStyle(colors: LogColorPreference): CSSProperties {
  const style: Record<string, string> = {};
  for (const key of LOG_COLOR_KEYS) {
    const color = colors[key];
    if (color) style[`--log-color-${key}`] = color;
  }
  return style as CSSProperties;
}

/** 当前主题下该项的实际颜色,供跟随主题时的取色器参考值使用。 */
export function themeLogColor(key: LogColorKey, root?: HTMLElement): string {
  const element = root ?? (typeof document === 'undefined' ? null : document.documentElement);
  const view = element?.ownerDocument.defaultView;
  if (!element || !view) return '#000000';
  const value = view.getComputedStyle(element).getPropertyValue(THEME_VARIABLES[key]);
  return normalizeHexColor(value) ?? '#000000';
}
