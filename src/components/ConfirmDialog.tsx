import { useEffect } from 'react';
import { useI18n } from '../i18n/I18nProvider';

interface Props {
  title: string;
  message: string;
  confirmLabel: string;
  onCancel: () => void;
  onConfirm: () => void;
}

export function ConfirmDialog({ title, message, confirmLabel, onCancel, onConfirm }: Props) {
  const { t } = useI18n();
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onCancel();
    };
    document.addEventListener('keydown', onKeyDown);
    return () => document.removeEventListener('keydown', onKeyDown);
  }, [onCancel]);

  return (
    <div className="update-modal-backdrop" onMouseDown={onCancel}>
      <div
        className="update-modal confirm-modal"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="confirm-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="confirm-modal-icon">!</div>
        <h2 id="confirm-title">{title}</h2>
        <p className="confirm-modal-message">{message}</p>
        <div className="update-modal-actions">
          <button className="settings-button secondary" onClick={onCancel} autoFocus>
            {t('common.cancel')}
          </button>
          <button className="settings-button danger" onClick={onConfirm}>
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
