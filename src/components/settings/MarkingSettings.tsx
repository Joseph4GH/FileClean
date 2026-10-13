import type { AutoMarkConfig } from '../../types';
import { useI18n } from '../../i18n';
import { Icon } from '../Icon';
import { RuleEditor } from '../RuleEditor';

interface Props {
  markConfig: AutoMarkConfig;
  onMarkConfig: (next: AutoMarkConfig) => void;
}

export function MarkingSettings({ markConfig, onMarkConfig }: Props) {
  const { t } = useI18n();
  const enabled = markConfig.rules.filter((r) => r.enabled).length;

  return (
    <div className="settings-form">
      <div className="alert info">
        <div className="row" style={{ gap: 6, marginBottom: 4 }}>
          <Icon name="check" size={13} />
          <span>
            {t('marking.banner', { n: markConfig.rules.length, m: enabled })}
          </span>
        </div>
        <small className="hint">{t('marking.bannerHint', { n: markConfig.keepPerGroup })}</small>
      </div>

      <RuleEditor config={markConfig} onChange={onMarkConfig} />
    </div>
  );
}
