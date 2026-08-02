import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';
import { JSDOM } from 'jsdom';
import {
  DEFAULT_UI_TEMPLATE,
  UI_TEMPLATE_STORAGE_KEY,
  applyUiTemplate,
  loadUiTemplate,
  saveUiTemplate,
} from './uiTemplate';

describe('界面模板偏好', () => {
  it('缺失或未知值安全回退原生模板', () => {
    const values = new Map<string, string>();
    const storage = { getItem: (key: string) => values.get(key) ?? null };

    assert.equal(loadUiTemplate(storage), DEFAULT_UI_TEMPLATE);
    values.set(UI_TEMPLATE_STORAGE_KEY, 'unknown');
    assert.equal(loadUiTemplate(storage), DEFAULT_UI_TEMPLATE);
  });

  it('保存选择后可在新的初始化过程恢复', () => {
    const values = new Map<string, string>();
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => values.set(key, value),
    };

    saveUiTemplate(storage, 'aurora');
    assert.equal(loadUiTemplate(storage), 'aurora');
    saveUiTemplate(storage, 'amber');
    assert.equal(loadUiTemplate(storage), 'amber');
    saveUiTemplate(storage, 'verdant');
    assert.equal(loadUiTemplate(storage), 'verdant');
  });

  it('存储不可用时回退或保留当前进程选择', () => {
    const brokenStorage = {
      getItem: () => {
        throw new Error('storage disabled');
      },
      setItem: () => {
        throw new Error('storage disabled');
      },
    };

    assert.equal(loadUiTemplate(brokenStorage), DEFAULT_UI_TEMPLATE);
    assert.doesNotThrow(() => saveUiTemplate(brokenStorage, 'aurora'));
  });

  it('即时把模板应用到根元素且不改动深浅配色', () => {
    const dom = new JSDOM('<!doctype html><html data-theme="dark"><body></body></html>');
    const root = dom.window.document.documentElement;

    applyUiTemplate(root, 'aurora');
    assert.equal(root.dataset.uiTemplate, 'aurora');
    assert.equal(root.dataset.theme, 'dark');

    applyUiTemplate(root, 'amber');
    assert.equal(root.dataset.uiTemplate, 'amber');
    assert.equal(root.dataset.theme, 'dark');

    applyUiTemplate(root, 'verdant');
    assert.equal(root.dataset.uiTemplate, 'verdant');
    assert.equal(root.dataset.theme, 'dark');
  });

  it('为两套丰富模板提供深浅变量、全局材质和减少动态效果降级', () => {
    const css = readFileSync(new URL('../styles.css', import.meta.url), 'utf8');
    assert.match(css, /:root\[data-ui-template='aurora'\]/);
    assert.match(css, /:root\[data-theme='light'\]\[data-ui-template='aurora'\]/);
    assert.match(css, /:root\[data-ui-template='amber'\]/);
    assert.match(css, /:root\[data-theme='light'\]\[data-ui-template='amber'\]/);
    assert.match(css, /--ui-template-accent-gradient:/);
    assert.match(css, /prefers-reduced-motion: reduce/);

    const templateRules = css.slice(css.indexOf('/* ===== 极光 / 琥珀全局组件材质 ===== */'));
    assert.doesNotMatch(templateRules, /--log-font-size|--log-gutter-width/);
    assert.doesNotMatch(templateRules, /font-family:\s*var\(--log-font\)/);
  });

  it('让日志选项卡与字段筛选模块呈现模板专属材质且不改变几何', () => {
    const css = readFileSync(new URL('../styles.css', import.meta.url), 'utf8');
    const moduleStart = css.indexOf('/* ===== 丰富模板：日志选项卡与字段筛选模块 ===== */');
    const moduleEnd = css.indexOf('/* ===== 应用整体三栏 + 顶栏 ===== */', moduleStart);
    const moduleRules = css.slice(moduleStart, moduleEnd);

    assert.notEqual(moduleStart, -1);
    assert.notEqual(moduleEnd, -1);
    assert.match(moduleRules, /:root\[data-ui-template='aurora'\] \.app \.log-tab\.active\s*\{/);
    assert.match(
      moduleRules,
      /:root\[data-ui-template='aurora'\] \.app \.log-field\.active \.log-field-button\s*\{/,
    );
    assert.match(
      moduleRules,
      /:root\[data-ui-template='aurora'\] \.app \.log-field-choice\.is-selected\s*\{/,
    );
    assert.match(moduleRules, /:root\[data-ui-template='amber'\] \.app \.log-tab\.active\s*\{/);
    assert.match(
      moduleRules,
      /:root\[data-ui-template='amber'\] \.app \.log-field\.active \.log-field-button\s*\{/,
    );
    assert.match(
      moduleRules,
      /:root\[data-ui-template='amber'\] \.app \.log-field-choice\.is-selected\s*\{/,
    );
    assert.doesNotMatch(
      moduleRules,
      /(?:^|[;{]\s*)(?:width|height|padding|margin|flex|font-size|font-family|line-height)\s*:/m,
    );
  });
});
