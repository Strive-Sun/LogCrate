import { useEffect, useRef } from 'react';
import { useI18n } from '../i18n/I18nProvider';

interface Props {
  onLater: () => void;
  onOpenSettings: () => void;
}

export function MacOsFileAccessDialog({ onLater, onOpenSettings }: Props) {
  const { t } = useI18n();
  const laterButton = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    laterButton.current?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onLater();
    };
    document.addEventListener('keydown', onKeyDown);
    return () => document.removeEventListener('keydown', onKeyDown);
  }, [onLater]);

  return (
    <div className="update-modal-backdrop">
      <div
        className="update-modal macos-access-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="macos-access-title"
      >
        <div className="macos-access-icon" aria-hidden="true">
          📁
        </div>
        <h2 id="macos-access-title">{t('macosAccess.title')}</h2>
        <p>{t('macosAccess.description')}</p>
        <ol>
          <li>{t('macosAccess.stepAdd')}</li>
          <li>{t('macosAccess.stepEnable')}</li>
          <li>{t('macosAccess.stepReturn')}</li>
        </ol>
        <p className="macos-access-note">{t('macosAccess.optional')}</p>
        <div className="update-modal-actions">
          <button ref={laterButton} className="settings-button secondary" onClick={onLater}>
            {t('macosAccess.later')}
          </button>
          <button className="settings-button primary" onClick={onOpenSettings}>
            {t('macosAccess.openSettings')}
          </button>
        </div>
      </div>
    </div>
  );
}
