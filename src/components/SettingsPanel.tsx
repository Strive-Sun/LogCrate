import type { AppUpdateInfo, AppUpdateProgress } from '../api';
import { formatBytes, type UpdateStatus } from '../util/update';
import { useI18n } from '../i18n/I18nProvider';

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
}

const busyStatuses: UpdateStatus[] = ['checking', 'downloading', 'installing'];

export function SettingsPanel(props: Props) {
  const { preference, setPreference, t } = useI18n();
  const busy = busyStatuses.includes(props.status);
  const progressLabel = props.progress
    ? props.progress.totalBytes
      ? `${formatBytes(props.progress.downloadedBytes)} / ${formatBytes(props.progress.totalBytes)}`
      : t('update.downloaded', { size: formatBytes(props.progress.downloadedBytes) })
    : '';

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
