import type { AppUpdateInfo } from '../api';
import { useI18n } from '../i18n/I18nProvider';

interface Props {
  update: AppUpdateInfo;
  onSkip: () => void;
  onDownload: () => void;
}

export function UpdateDialog({ update, onSkip, onDownload }: Props) {
  const { t } = useI18n();
  return (
    <div className="update-modal-backdrop">
      <div
        className="update-modal"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="update-title"
      >
        <div className="update-modal-icon">⬆</div>
        <h2 id="update-title">{t('update.available', { version: update.version })}</h2>
        <p>{t('update.dialogText', { current: update.currentVersion })}</p>
        {update.body && <div className="update-notes">{update.body}</div>}
        <div className="update-modal-actions">
          <button className="settings-button secondary" onClick={onSkip}>
            {t('update.skip')}
          </button>
          <button className="settings-button primary" onClick={onDownload} autoFocus>
            {t('update.download')}
          </button>
        </div>
      </div>
    </div>
  );
}
