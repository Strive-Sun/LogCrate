import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';
import { JSDOM } from 'jsdom';
import {
  LOG_COLOR_STORAGE_KEY,
  loadLogColors,
  logColorStyle,
  normalizeHexColor,
  saveLogColors,
  themeLogColor,
  withLogColor,
} from './logColors';

function memoryStorage() {
  const values = new Map<string, string>();
  return {
    values,
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => void values.set(key, value),
  };
}

describe('日志颜色偏好', () => {
  it('缺失、损坏或存储不可用时全部跟随主题', () => {
    const storage = memoryStorage();
    assert.deepEqual(loadLogColors(storage), {});
    storage.values.set(LOG_COLOR_STORAGE_KEY, '{bad');
    assert.deepEqual(loadLogColors(storage), {});
    storage.values.set(LOG_COLOR_STORAGE_KEY, 'null');
    assert.deepEqual(loadLogColors(storage), {});
    assert.deepEqual(
      loadLogColors({
        getItem: () => {
          throw new Error('denied');
        },
      }),
      {},
    );
  });

  it('逐项校验并规范为小写，非法项跟随主题', () => {
    const storage = memoryStorage();
    storage.values.set(
      LOG_COLOR_STORAGE_KEY,
      JSON.stringify({
        body: '#1A1A1A',
        info: 'black',
        warn: '#abc',
        error: '#ff0000',
        fatal: '#123456',
      }),
    );
    assert.deepEqual(loadLogColors(storage), { body: '#1a1a1a', error: '#ff0000' });
  });

  it('保存后可恢复，写入失败不抛出', () => {
    const storage = memoryStorage();
    saveLogColors(storage, { info: '#000000' });
    assert.deepEqual(loadLogColors(storage), { info: '#000000' });
    assert.doesNotThrow(() =>
      saveLogColors(
        {
          setItem: () => {
            throw new Error('quota');
          },
        },
        { info: '#000000' },
      ),
    );
  });

  it('设置与恢复单项颜色不影响其它项', () => {
    const start = { error: '#ff0000' };
    const withInfo = withLogColor(start, 'info', '#000000');
    assert.deepEqual(withInfo, { error: '#ff0000', info: '#000000' });
    assert.deepEqual(start, { error: '#ff0000' }, 'input is not mutated');
    assert.deepEqual(withLogColor(withInfo, 'info', null), { error: '#ff0000' });
    assert.deepEqual(withLogColor(withInfo, 'info', 'not-a-color'), { error: '#ff0000' });
  });

  it('只为自定义项输出面板变量', () => {
    assert.deepEqual(logColorStyle({}), {});
    assert.deepEqual(logColorStyle({ info: '#000000', body: '#222222' }), {
      '--log-color-body': '#222222',
      '--log-color-info': '#000000',
    });
  });

  it('规范主题颜色格式供取色器使用', () => {
    assert.equal(normalizeHexColor(' #5a9 '), '#55aa99');
    assert.equal(normalizeHexColor('#D33'), '#dd3333');
    assert.equal(normalizeHexColor('#1c1c20'), '#1c1c20');
    assert.equal(normalizeHexColor('rgb(0, 136, 119)'), '#008877');
    assert.equal(normalizeHexColor('rgb(300, 0, 0)'), null);
    assert.equal(normalizeHexColor('green'), null);
  });

  it('跟随主题时读取当前主题变量的实际颜色', () => {
    const dom = new JSDOM('<!doctype html><html><body></body></html>');
    const root = dom.window.document.documentElement;
    root.style.setProperty('--info', '#5a9');
    assert.equal(themeLogColor('info', root), '#55aa99');
    assert.equal(themeLogColor('error', root), '#000000');
  });

  it('样式表在未自定义时回退到主题颜色', () => {
    const css = readFileSync(new URL('../styles.css', import.meta.url), 'utf8');
    assert.match(css, /\.log-line \.txt \{[^}]*color: var\(--log-color-body, currentColor\);/);
    for (const [level, key, theme] of [
      ['ERROR', 'error', 'error'],
      ['WARN', 'warn', 'warn'],
      ['INFO', 'info', 'info'],
      ['TRACE', 'debug', 'debug'],
    ]) {
      assert.match(
        css,
        new RegExp(
          `\\.lvl-${level} \\{\\s*color: var\\(--log-color-${key}, var\\(--${theme}\\)\\);`,
        ),
      );
    }
  });
});
