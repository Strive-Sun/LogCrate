import type { AppUpdateInfo, AppUpdateProgress } from '../api';
import { formatBytes, type UpdateStatus } from '../util/update';

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
}

const busyStatuses: UpdateStatus[] = ['checking', 'downloading', 'installing'];

export function SettingsPanel(props: Props) {
  const busy = busyStatuses.includes(props.status);
  const progressLabel = props.progress
    ? props.progress.totalBytes
      ? `${formatBytes(props.progress.downloadedBytes)} / ${formatBytes(props.progress.totalBytes)}`
      : `${formatBytes(props.progress.downloadedBytes)} 已下载`
    : '';

  return (
    <div className="pop settings-pop" role="dialog" aria-label="设置">
      <div className="pop-head">
        <span>设置</span>
        <button className="settings-close" onClick={props.onClose} aria-label="关闭设置">
          ×
        </button>
      </div>

      <div className="settings-section">
        <div className="settings-row">
          <div>
            <div className="settings-label">当前版本</div>
            <div className="settings-hint">LogPeek 桌面应用</div>
          </div>
          <code className="version-value">v{props.currentVersion}</code>
        </div>

        <label className="settings-row settings-toggle-row">
          <div>
            <div className="settings-label">启动时自动检查更新</div>
            <div className="settings-hint">仅发现新版本时提示</div>
          </div>
          <input
            type="checkbox"
            checked={props.autoCheck}
            onChange={(event) => props.onAutoCheckChange(event.target.checked)}
          />
        </label>
      </div>

      <div className="settings-section update-section" aria-live="polite">
        <div className="update-row">
          <div>
            <div className="settings-label">软件更新</div>
            <div className="settings-hint">通过签名验证后自动安装</div>
          </div>
          <button className="settings-button" disabled={busy} onClick={props.onCheck}>
            {props.status === 'checking' ? '检查中…' : '检查更新'}
          </button>
        </div>

        {props.status === 'up-to-date' && (
          <div className="update-message success">当前已是最新版本</div>
        )}
        {props.status === 'available' && props.update && (
          <div className="update-card">
            <div className="update-version">发现新版本 v{props.update.version}</div>
            <div className="settings-hint">下载完成后将自动安装并重启应用</div>
            <div className="update-actions">
              <button className="settings-button secondary" onClick={props.onSkip}>
                跳过此版本
              </button>
              <button className="settings-button primary" onClick={props.onDownload}>
                下载更新
              </button>
            </div>
          </div>
        )}
        {(props.status === 'downloading' || props.status === 'installing') && props.progress && (
          <div className="update-progress-wrap">
            <div className="update-progress-label">
              <span>{props.status === 'installing' ? '正在安装…' : '正在下载…'}</span>
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
              aria-label={props.status === 'installing' ? '安装更新' : '下载更新'}
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
          <div className="update-message success">更新已安装，正在重启…</div>
        )}
        {props.status === 'error' && (
          <div className="update-message error">更新失败：{props.error ?? '未知错误'}</div>
        )}
      </div>
    </div>
  );
}
