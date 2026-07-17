import type { AppUpdateInfo } from '../api';

interface Props {
  update: AppUpdateInfo;
  onSkip: () => void;
  onDownload: () => void;
}

export function UpdateDialog({ update, onSkip, onDownload }: Props) {
  return (
    <div className="update-modal-backdrop">
      <div
        className="update-modal"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="update-title"
      >
        <div className="update-modal-icon">⬆</div>
        <h2 id="update-title">发现新版本 v{update.version}</h2>
        <p>当前版本 v{update.currentVersion}。下载完成后将自动验证签名、安装更新并重启应用。</p>
        {update.body && <div className="update-notes">{update.body}</div>}
        <div className="update-modal-actions">
          <button className="settings-button secondary" onClick={onSkip}>
            跳过此版本
          </button>
          <button className="settings-button primary" onClick={onDownload} autoFocus>
            下载更新
          </button>
        </div>
      </div>
    </div>
  );
}
