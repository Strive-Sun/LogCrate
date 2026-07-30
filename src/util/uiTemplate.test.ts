import assert from 'node:assert/strict';
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
  });
});
