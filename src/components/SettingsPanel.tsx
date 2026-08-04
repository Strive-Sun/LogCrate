import { useEffect, useState } from 'react';
import {
  api,
  type AiProviderConfig,
  type AppUpdateInfo,
  type AppUpdateProgress,
  type FileSearchFeatureState,
} from '../api';
import { formatBytes, type UpdateStatus } from '../util/update';
import { useI18n } from '../i18n/I18nProvider';
import type { UiTemplate } from '../util/uiTemplate';

interface Props {
  currentVersion: string;
  autoCheck: boolean;
  status: UpdateStatus;
  update: AppUpdateInfo | null;
  progress: AppUpdateProgress | null;
  error: string | null;
  onAutoCheckChange: (enabled: boolean) => void;
  onCheck: () => void;
  onSkip: () => void;
  onDownload: () => void;
  onClose: () => void;
  macOsFileAccessSupported: boolean;
  onOpenMacOsFileAccessSettings: () => void;
  searchFeature: FileSearchFeatureState | null;
  searchPreferenceSaving: boolean;
  onSearchEnabledChange: (enabled: boolean) => void;
  uiTemplate: UiTemplate;
  onUiTemplateChange: (template: UiTemplate) => void;
}

const busyStatuses: UpdateStatus[] = ['checking', 'downloading', 'installing'];
const templateOptions = [
  {
    value: 'native',
    nameKey: 'settings.template.native',
    hintKey: 'settings.template.nativeHint',
  },
  {
    value: 'aurora',
    nameKey: 'settings.template.aurora',
    hintKey: 'settings.template.auroraHint',
  },
  {
    value: 'amber',
    nameKey: 'settings.template.amber',
    hintKey: 'settings.template.amberHint',
  },
  {
    value: 'verdant',
    nameKey: 'settings.template.verdant',
    hintKey: 'settings.template.verdantHint',
  },
] as const;

function isNonLocalHttp(value: string): boolean {
  try {
    const url = new URL(value);
    return (
      url.protocol === 'http:' && !['localhost', '127.0.0.1', '::1', '[::1]'].includes(url.hostname)
    );
  } catch {
    return false;
  }
}

export function SettingsPanel(props: Props) {
  const { preference, setPreference, t } = useI18n();
  const busy = busyStatuses.includes(props.status);
  const [providers, setProviders] = useState<AiProviderConfig[]>([]);
  const [provider, setProvider] = useState<AiProviderConfig>({
    id: '',
    name: '',
    baseUrl: 'https://api.openai.com/v1',
    model: '',
    keyConfigured: false,
    protocol: 'chatCompletions',
    endpointMode: 'base',
    allowInsecureHttp: false,
  });
  const [apiKey, setApiKey] = useState('');
  const [providerMessage, setProviderMessage] = useState<string | null>(null);
  const [providerBusy, setProviderBusy] = useState(false);
  const [providerExpanded, setProviderExpanded] = useState(false);
  useEffect(() => {
    void api
      .listAiProviders()
      .then(setProviders)
      .catch(() => setProviderMessage('无法读取 AI 供应商配置'));
  }, []);
  const saveProvider = async () => {
    if (
      !provider.id.trim() ||
      !provider.name.trim() ||
      !provider.model.trim() ||
      !provider.baseUrl.trim()
    ) {
      setProviderMessage('请填写供应商名称、端点和模型');
      return;
    }
    setProviderBusy(true);
    setProviderMessage(null);
    try {
      const saved = await api.saveAiProvider(
        {
          ...provider,
          id: provider.id.trim(),
          name: provider.name.trim(),
          model: provider.model.trim(),
          baseUrl: provider.baseUrl.trim(),
        },
        apiKey,
      );
      setProviders((items) => [...items.filter((item) => item.id !== saved.id), saved]);
      setProvider({ ...saved });
      setApiKey('');
      setProviderMessage('已保存（密钥存储在系统密钥链）');
    } catch (error) {
      setProviderMessage(error instanceof Error ? error.message : '保存失败');
    } finally {
      setProviderBusy(false);
    }
  };
  const progressLabel = props.progress
    ? props.progress.totalBytes
      ? `${formatBytes(props.progress.downloadedBytes)} / ${formatBytes(props.progress.totalBytes)}`
      : t('update.downloaded', { size: formatBytes(props.progress.downloadedBytes) })
    : '';

  const onTemplateKeyDown = (event: React.KeyboardEvent<HTMLInputElement>, index: number) => {
    const direction =
      event.key === 'ArrowRight' || event.key === 'ArrowDown'
        ? 1
        : event.key === 'ArrowLeft' || event.key === 'ArrowUp'
          ? -1
          : 0;
    if (!direction) return;
    event.preventDefault();
    const nextIndex = (index + direction + templateOptions.length) % templateOptions.length;
    props.onUiTemplateChange(templateOptions[nextIndex].value);
    const inputs = event.currentTarget
      .closest('.ui-template-options')
      ?.querySelectorAll<HTMLInputElement>('input[type="radio"]');
    inputs?.[nextIndex]?.focus();
  };

  return (
    <div className="pop settings-pop" role="dialog" aria-label={t('settings.title')}>
      <div className="pop-head">
        <span>{t('settings.title')}</span>
        <button className="settings-close" onClick={props.onClose} aria-label={t('settings.close')}>
          ×
        </button>
      </div>

      <div className="settings-section">
        <div className="settings-row">
          <div>
            <div className="settings-label">{t('settings.version')}</div>
            <div className="settings-hint">{t('settings.appHint')}</div>
          </div>
          <code className="version-value">v{props.currentVersion}</code>
        </div>

        <button
          type="button"
          className="settings-row settings-disclosure"
          aria-expanded={providerExpanded}
          aria-controls="ai-provider-settings"
          onClick={() => setProviderExpanded((expanded) => !expanded)}
        >
          <div>
            <div className="settings-label">AI 供应商</div>
            <div className="settings-hint">
              {providers.length > 0
                ? `已配置 ${providers.length} 个供应商`
                : '配置 AI 日志分析服务'}
            </div>
          </div>
          <span className="settings-disclosure-icon" aria-hidden="true">
            {providerExpanded ? '▾' : '›'}
          </span>
        </button>

        {providerExpanded && (
          <div id="ai-provider-settings" className="settings-disclosure-content">
            <div className="settings-hint">
              日志仅在你确认分析后发送到所配置的第三方端点；API Key 不会写入配置文件。
            </div>
            {providers.map((item) => (
              <div className="settings-row" key={item.id}>
                <div>
                  <div className="settings-label">{item.name}</div>
                  <div className="settings-hint">
                    {item.baseUrl} · {item.model} ·{' '}
                    {item.protocol === 'responses' ? 'Responses' : 'Chat Completions'} ·{' '}
                    {item.keyConfigured ? '密钥已配置' : '未配置密钥'}
                    {item.allowInsecureHttp ? ' · 不安全 HTTP' : ''}
                  </div>
                </div>
                <span>
                  <button
                    className="settings-button"
                    onClick={() => {
                      setProvider({ ...item });
                      setApiKey('');
                    }}
                  >
                    编辑
                  </button>{' '}
                  <button
                    className="settings-button"
                    onClick={() =>
                      void api
                        .testAiProvider(item.id)
                        .then(() => setProviderMessage('连接成功'))
                        .catch(() => setProviderMessage('连接失败'))
                    }
                  >
                    测试
                  </button>{' '}
                  <button
                    className="settings-button"
                    onClick={() =>
                      void api
                        .deleteAiProvider(item.id)
                        .then(() => setProviders((all) => all.filter((p) => p.id !== item.id)))
                    }
                  >
                    删除
                  </button>
                </span>
              </div>
            ))}
            <div className="settings-provider-form">
              <select
                className="settings-input"
                aria-label="预设供应商"
                onChange={(e) => {
                  const presets: Record<string, Partial<AiProviderConfig>> = {
                    openai: {
                      name: 'OpenAI',
                      baseUrl: 'https://api.openai.com/v1',
                      model: 'gpt-4o-mini',
                      protocol: 'chatCompletions',
                      endpointMode: 'base',
                      allowInsecureHttp: false,
                    },
                    deepseek: {
                      name: 'DeepSeek',
                      baseUrl: 'https://api.deepseek.com/v1',
                      model: 'deepseek-chat',
                      protocol: 'chatCompletions',
                      endpointMode: 'base',
                      allowInsecureHttp: false,
                    },
                    qwen: {
                      name: '通义千问',
                      baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
                      model: 'qwen-plus',
                      protocol: 'chatCompletions',
                      endpointMode: 'base',
                      allowInsecureHttp: false,
                    },
                    openrouter: {
                      name: 'OpenRouter',
                      baseUrl: 'https://openrouter.ai/api/v1',
                      model: 'openai/gpt-4o-mini',
                      protocol: 'chatCompletions',
                      endpointMode: 'base',
                      allowInsecureHttp: false,
                    },
                  };
                  const preset = presets[e.target.value];
                  if (preset)
                    setProvider((current) => ({
                      ...current,
                      ...preset,
                      id: current.id || e.target.value,
                    }));
                }}
                defaultValue=""
              >
                <option value="">选择预设供应商（可自定义）</option>
                <option value="openai">OpenAI</option>
                <option value="deepseek">DeepSeek</option>
                <option value="qwen">通义千问</option>
                <option value="openrouter">OpenRouter</option>
              </select>
              <input
                className="settings-input"
                placeholder="供应商 ID"
                value={provider.id}
                onChange={(e) => setProvider({ ...provider, id: e.target.value })}
              />
              <input
                className="settings-input"
                placeholder="名称"
                value={provider.name}
                onChange={(e) => setProvider({ ...provider, name: e.target.value })}
              />
              <div className="settings-label">API 请求地址</div>
              <input
                className="settings-input"
                aria-label="API 请求地址"
                placeholder={
                  provider.endpointMode === 'full'
                    ? '完整请求 URL，例如 https://example.com/v1/responses'
                    : '基础地址，例如 https://example.com/v1'
                }
                value={provider.baseUrl}
                onChange={(e) => {
                  const baseUrl = e.target.value;
                  setProvider({
                    ...provider,
                    baseUrl,
                    allowInsecureHttp:
                      baseUrl === provider.baseUrl && isNonLocalHttp(baseUrl)
                        ? provider.allowInsecureHttp
                        : false,
                  });
                }}
              />
              <select
                className="settings-input"
                aria-label="API 协议"
                value={provider.protocol}
                onChange={(event) =>
                  setProvider({
                    ...provider,
                    protocol: event.target.value as AiProviderConfig['protocol'],
                  })
                }
              >
                <option value="responses">OpenAI Responses</option>
                <option value="chatCompletions">OpenAI Chat Completions</option>
              </select>
              <label className="settings-provider-option">
                <input
                  type="checkbox"
                  checked={provider.endpointMode === 'full'}
                  onChange={(event) =>
                    setProvider({
                      ...provider,
                      endpointMode: event.target.checked ? 'full' : 'base',
                    })
                  }
                />
                使用完整 URL（不自动拼接协议路径）
              </label>
              {isNonLocalHttp(provider.baseUrl) && (
                <div className="settings-insecure-warning">
                  <strong>此地址未使用 HTTPS。</strong>
                  API Key 和所选日志可能在内网中以明文传输，建议优先使用 HTTPS。
                  <label className="settings-provider-option">
                    <input
                      type="checkbox"
                      checked={provider.allowInsecureHttp}
                      onChange={(event) => {
                        if (!event.target.checked) {
                          setProvider({ ...provider, allowInsecureHttp: false });
                          return;
                        }
                        if (
                          window.confirm(
                            '该端点使用不安全 HTTP，API Key 和日志内容可能被网络中的其他设备读取。是否仅为当前供应商允许此地址？',
                          )
                        ) {
                          setProvider({ ...provider, allowInsecureHttp: true });
                        }
                      }}
                    />
                    我了解风险，仅允许当前供应商使用此 HTTP 地址
                  </label>
                </div>
              )}
              <input
                className="settings-input"
                placeholder="模型"
                value={provider.model}
                onChange={(e) => setProvider({ ...provider, model: e.target.value })}
              />
              <input
                className="settings-input"
                type="password"
                autoComplete="new-password"
                placeholder={provider.keyConfigured ? 'API Key 已配置（留空保持不变）' : 'API Key'}
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
              />
              <button
                className="settings-button primary"
                disabled={providerBusy}
                onClick={() => void saveProvider()}
              >
                {providerBusy ? '保存中…' : '保存供应商'}
              </button>
            </div>
            {providerMessage && (
              <div className="settings-hint" role="status">
                {providerMessage}
              </div>
            )}
          </div>
        )}
      </div>

      <div className="settings-section">
        <label className="settings-row">
          <div>
            <div className="settings-label">{t('settings.language')}</div>
            <div className="settings-hint">{t('settings.languageHint')}</div>
          </div>
          <select
            className="language-select"
            value={preference}
            onChange={(e) => setPreference(e.target.value as typeof preference)}
          >
            <option value="system">{t('settings.language.system')}</option>
            <option value="zh-CN">{t('settings.language.zhCN')}</option>
            <option value="en">{t('settings.language.en')}</option>
          </select>
        </label>

        <label className="settings-row settings-toggle-row">
          <div>
            <div className="settings-label">{t('settings.autoUpdate')}</div>
            <div className="settings-hint">{t('settings.autoUpdateHint')}</div>
          </div>
          <input
            type="checkbox"
            checked={props.autoCheck}
            onChange={(event) => props.onAutoCheckChange(event.target.checked)}
          />
        </label>

        <label className="settings-row settings-toggle-row">
          <div>
            <div className="settings-label">{t('settings.searchEnabled')}</div>
            <div className="settings-hint">{t('settings.searchEnabledHint')}</div>
            {props.searchFeature &&
              props.searchFeature.currentEnabled !== props.searchFeature.nextLaunchEnabled && (
                <div className="settings-hint">
                  {t(
                    props.searchFeature.nextLaunchEnabled
                      ? 'settings.searchPendingEnable'
                      : 'settings.searchPendingDisable',
                  )}
                </div>
              )}
          </div>
          <input
            type="checkbox"
            aria-label={t('settings.searchEnabled')}
            checked={props.searchFeature?.nextLaunchEnabled ?? false}
            disabled={!props.searchFeature || props.searchPreferenceSaving}
            onChange={(event) => props.onSearchEnabledChange(event.target.checked)}
          />
        </label>
      </div>

      <div className="settings-section settings-template-section">
        <div className="settings-template-heading">
          <div className="settings-label">{t('settings.template')}</div>
          <div className="settings-hint">{t('settings.templateHint')}</div>
        </div>
        <div className="ui-template-options" role="radiogroup" aria-label={t('settings.template')}>
          {templateOptions.map((option, index) => {
            const selected = props.uiTemplate === option.value;
            return (
              <label
                className={'ui-template-card' + (selected ? ' is-selected' : '')}
                key={option.value}
              >
                <input
                  className="ui-template-input"
                  type="radio"
                  name="ui-template"
                  value={option.value}
                  checked={selected}
                  onChange={() => props.onUiTemplateChange(option.value)}
                  onKeyDown={(event) => onTemplateKeyDown(event, index)}
                />
                <span className={`ui-template-preview is-${option.value}`} aria-hidden="true">
                  <span className="ui-template-preview-top" />
                  <span className="ui-template-preview-body">
                    <i />
                    <span>
                      <i />
                      <i />
                      <i />
                    </span>
                  </span>
                </span>
                <span className="ui-template-card-copy">
                  <strong>{t(option.nameKey)}</strong>
                  <small>{t(option.hintKey)}</small>
                </span>
                <span className="ui-template-check" aria-hidden="true">
                  {selected ? '✓' : ''}
                </span>
              </label>
            );
          })}
        </div>
      </div>

      {props.macOsFileAccessSupported && (
        <div className="settings-section">
          <div className="settings-row">
            <div>
              <div className="settings-label">{t('macosAccess.settingsLabel')}</div>
              <div className="settings-hint">{t('macosAccess.settingsHint')}</div>
            </div>
            <button className="settings-button" onClick={props.onOpenMacOsFileAccessSettings}>
              {t('macosAccess.openSettings')}
            </button>
          </div>
        </div>
      )}

      <div className="settings-section update-section" aria-live="polite">
        <div className="update-row">
          <div>
            <div className="settings-label">{t('settings.softwareUpdate')}</div>
            <div className="settings-hint">{t('settings.softwareUpdateHint')}</div>
          </div>
          <button className="settings-button" disabled={busy} onClick={props.onCheck}>
            {props.status === 'checking' ? t('update.checking') : t('update.check')}
          </button>
        </div>

        {props.status === 'up-to-date' && (
          <div className="update-message success">{t('update.latest')}</div>
        )}
        {props.status === 'available' && props.update && (
          <div className="update-card">
            <div className="update-version">
              {t('update.available', { version: props.update.version })}
            </div>
            <div className="settings-hint">{t('update.installHint')}</div>
            <div className="update-actions">
              <button className="settings-button secondary" onClick={props.onSkip}>
                {t('update.skip')}
              </button>
              <button className="settings-button primary" onClick={props.onDownload}>
                {t('update.download')}
              </button>
            </div>
          </div>
        )}
        {(props.status === 'downloading' || props.status === 'installing') && props.progress && (
          <div className="update-progress-wrap">
            <div className="update-progress-label">
              <span>
                {props.status === 'installing' ? t('update.installing') : t('update.downloading')}
              </span>
              <span>
                {props.progress.percent === undefined
                  ? progressLabel
                  : `${props.progress.percent}%`}
              </span>
            </div>
            <div
              className={
                'update-progress' + (props.progress.percent === undefined ? ' indeterminate' : '')
              }
              role="progressbar"
              aria-label={
                props.status === 'installing' ? t('update.installAria') : t('update.downloadAria')
              }
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={props.progress.percent}
            >
              <span style={{ width: `${props.progress.percent ?? 35}%` }} />
            </div>
            {progressLabel && <div className="settings-hint progress-bytes">{progressLabel}</div>}
          </div>
        )}
        {props.status === 'installed' && (
          <div className="update-message success">{t('update.installed')}</div>
        )}
        {props.status === 'error' && (
          <div className="update-message error">
            {t('update.failed', { error: props.error ?? t('common.unknown') })}
          </div>
        )}
      </div>
    </div>
  );
}
