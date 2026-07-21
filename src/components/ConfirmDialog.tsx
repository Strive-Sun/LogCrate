import { useI18n } from '../i18n/I18nProvider';

interface Props {
  title: string;
  message: string;
  confirmLabel: string;
  cancelLabel?: string;
  showCancel?: boolean;
  danger?: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

export function ConfirmDialog({
  title,
  message,
  confirmLabel,
  cancelLabel,
  showCancel = true,
  danger = true,
  onCancel,
  onConfirm,
}: Props) {
  const { t } = useI18n();

  return (
    <div className="update-modal-backdrop">
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
          {showCancel && (
            <button className="settings-button secondary" onClick={onCancel} autoFocus>
              {cancelLabel ?? t('common.cancel')}
            </button>
          )}
          <button
            className={'settings-button' + (danger ? ' danger' : '')}
            onClick={onConfirm}
            autoFocus={!showCancel}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
