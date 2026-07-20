interface Props {
  onAddDir: () => void;
}

export function EmptyState({ onAddDir }: Props) {
  const { t } = useI18n();
  return (
    <div className="col col-content">
      <div className="empty-state">
        <div className="big">📂</div>
        <div className="title">{t('empty.title')}</div>
        <div className="desc">{t('empty.description')}</div>
        <button className="cta" onClick={onAddDir}>
          {t('tree.add')}
        </button>
      </div>
    </div>
  );
}
import { useI18n } from '../i18n/I18nProvider';
