import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';
import {
  DEFAULT_LOG_FONT,
  LOG_FONT_SIZES,
  LOG_FONT_STORAGE_KEY,
  loadLogFont,
  logFontStyle,
  logGutterWidth,
  logRowHeight,
  saveLogFont,
} from './logFont';

function memoryStorage() {
  const values = new Map<string, string>();
  return {
    values,
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => void values.set(key, value),
  };
}

describe('日志字体偏好', () => {
  it('缺失、损坏或存储不可用时使用默认等宽 12px', () => {
    const storage = memoryStorage();
    assert.deepEqual(loadLogFont(storage), DEFAULT_LOG_FONT);
    storage.values.set(LOG_FONT_STORAGE_KEY, '{not json');
    assert.deepEqual(loadLogFont(storage), DEFAULT_LOG_FONT);
    assert.deepEqual(
      loadLogFont({
        getItem: () => {
          throw new Error('denied');
        },
      }),
      DEFAULT_LOG_FONT,
    );
  });

  it('字体与字号逐项回退', () => {
    const storage = memoryStorage();
    storage.values.set(LOG_FONT_STORAGE_KEY, JSON.stringify({ family: 'comic', size: 15 }));
    assert.deepEqual(loadLogFont(storage), { family: 'default', size: 15 });
    storage.values.set(LOG_FONT_STORAGE_KEY, JSON.stringify({ family: 'yahei', size: 17 }));
    assert.deepEqual(loadLogFont(storage), { family: 'yahei', size: 12 });
    storage.values.set(LOG_FONT_STORAGE_KEY, JSON.stringify({ family: 'consolas', size: '15' }));
    assert.deepEqual(loadLogFont(storage), { family: 'consolas', size: 12 });
  });

  it('保存后可恢复，写入失败不抛出', () => {
    const storage = memoryStorage();
    saveLogFont(storage, { family: 'yahei', size: 16 });
    assert.deepEqual(loadLogFont(storage), { family: 'yahei', size: 16 });
    assert.doesNotThrow(() =>
      saveLogFont(
        {
          setItem: () => {
            throw new Error('quota');
          },
        },
        { family: 'lucida', size: 14 },
      ),
    );
  });

  it('默认字号的行高与槽宽保持原值，其余档位随字号缩放', () => {
    assert.equal(logRowHeight(12), 18);
    assert.equal(logGutterWidth(12), 72);
    assert.equal(logRowHeight(15), 23);
    assert.equal(logRowHeight(20), 30);
    for (const size of LOG_FONT_SIZES) {
      assert.ok(logRowHeight(size) >= size + 4, `row height leaves leading for ${size}px`);
    }
  });

  it('默认字体栈与样式表中的原有 --log-font 一致，预设都带通用回退', () => {
    const css = readFileSync(new URL('../styles.css', import.meta.url), 'utf8');
    const original = /--log-font:\s*([^;]+);/.exec(css)?.[1].trim();
    const defaults = logFontStyle(DEFAULT_LOG_FONT) as Record<string, string>;
    assert.equal(defaults['--log-font'], original);
    assert.equal(defaults['--log-font-size'], '12px');
    assert.equal(defaults['--log-gutter-width'], '72px');
    for (const family of ['default', 'consolas', 'lucida', 'yahei'] as const) {
      const stack = (logFontStyle({ family, size: 12 }) as Record<string, string>)['--log-font'];
      assert.match(stack, /(monospace|sans-serif)$/);
    }
  });
});
