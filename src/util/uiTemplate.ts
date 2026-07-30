export type UiTemplate = 'native' | 'aurora' | 'amber';

export const UI_TEMPLATE_STORAGE_KEY = 'logcrate.uiTemplate.v1';
export const DEFAULT_UI_TEMPLATE: UiTemplate = 'native';

export function isUiTemplate(value: unknown): value is UiTemplate {
  return value === 'native' || value === 'aurora' || value === 'amber';
}

export function loadUiTemplate(storage: Pick<Storage, 'getItem'>): UiTemplate {
  try {
    const value = storage.getItem(UI_TEMPLATE_STORAGE_KEY);
    return isUiTemplate(value) ? value : DEFAULT_UI_TEMPLATE;
  } catch {
    return DEFAULT_UI_TEMPLATE;
  }
}

export function saveUiTemplate(storage: Pick<Storage, 'setItem'>, template: UiTemplate): void {
  try {
    storage.setItem(UI_TEMPLATE_STORAGE_KEY, template);
  } catch {
    // WebView 存储不可用时仍保留当前进程内选择。
  }
}

export function applyUiTemplate(root: HTMLElement, template: UiTemplate): void {
  root.dataset.uiTemplate = template;
}
