import type { ScanOptions, SystemInfo } from '../../types';
import { useI18n, type DictKey } from '../../i18n';
import { Icon } from '../Icon';

interface Props {
  options: ScanOptions;
  patch: (value: Partial<ScanOptions>) => void;
  disabled: boolean;
  systemInfo: SystemInfo | null;
}

export function SafetySettings({ options, patch, disabled, systemInfo }: Props) {
  const { t } = useI18n();
  const platform = systemInfo?.platform ?? 'macos';
  const hintKey: DictKey =
    platform === 'macos' || platform === 'windows'
      ? (`safety.protectHint.${platform}` as DictKey)
      : 'safety.protectHint.default';

  return (
    <div className="settings-form">
      <div className="card">
        <label className="check">
          <input
            type="checkbox"
            checked={options.protectSystemDirs}
            onChange={(e) => patch({ protectSystemDirs: e.target.checked })}
            disabled={disabled}
          />
          <span>
            {t('safety.protectLock')}
            <small>{t(hintKey)}</small>
          </span>
        </label>
        <small className="hint" style={{ paddingLeft: 21 }}>
          {t('safety.protectHintDetail')}
        </small>
      </div>

      {options.protectSystemDirs === false && (
        <div className="alert warn" style={{ marginTop: 10 }}>
          <strong>{t('safety.lockOff')}</strong>
          <div style={{ marginTop: 4 }}>
            {t('safety.lockOffDetail')}
          </div>
        </div>
      )}

      <div className="divider" />

      <div className="card">
        <label className="check">
          <input
            type="checkbox"
            checked={options.checkReferences}
            onChange={(e) => patch({ checkReferences: e.target.checked })}
            disabled={disabled}
          />
          <span>
            {t('safety.refCheck')}
            <small>{t('safety.refCheckHint')}</small>
          </span>
        </label>
        <small className="hint" style={{ paddingLeft: 21 }}>
          {t('safety.refCheckDetail')}
        </small>
      </div>

      <div className="divider" />

      <div className="field">
        <span>{t('safety.redLines')}</span>
        <ul className="steps">
          <li>{t('safety.red1')}</li>
          <li>{t('safety.red2')}</li>
          <li>{t('safety.red3')}</li>
          <li>{t('safety.red4')}</li>
        </ul>
        <small className="hint">
          <Icon name="shield" size={11} /> {t('safety.redHint')}
        </small>
      </div>
    </div>
  );
}
