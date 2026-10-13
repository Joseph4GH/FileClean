import { algorithmOptions } from '../../i18n/meta';
import { useI18n } from '../../i18n';
import type { ScanOptions } from '../../types';
import { Icon } from '../Icon';

interface Props {
  options: ScanOptions;
  patch: (value: Partial<ScanOptions>) => void;
  disabled: boolean;
}

export function AlgorithmSettings({ options, patch, disabled }: Props) {
  const { t } = useI18n();
  const options_ = algorithmOptions();
  const selected = options_.find((o) => o.value === options.algorithm);

  return (
    <div className="settings-form">
      <label className="field">
        <span>{t('algo.title')}</span>
        <select
          value={options.algorithm}
          onChange={(e) => patch({ algorithm: e.target.value as ScanOptions['algorithm'] })}
          disabled={disabled}
        >
          {options_.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
        {selected && <small className="hint">{selected.hint}</small>}
      </label>

      <div className="alert info">
        <div className="row" style={{ gap: 6, marginBottom: 4 }}>
          <Icon name="sliders" size={13} />
          <span>{t('algo.threeStage')}</span>
        </div>
        <ol className="steps">
          <li>{t('algo.step1')}</li>
          <li>{t('algo.step2')}</li>
          <li>{t('algo.step3')}</li>
        </ol>
        <small className="hint">{t('algo.stepHint')}</small>
      </div>

      <div className="divider" />

      <div className="field">
        <span>{t('algo.contentMatch')}</span>
        <label className="check">
          <input
            type="checkbox"
            checked={options.rawJpegPairing}
            onChange={(e) => patch({ rawJpegPairing: e.target.checked })}
            disabled={disabled}
          />
          <span>
            {t('algo.rawJpeg')}
            <small>{t('algo.rawJpegHint')}</small>
          </span>
        </label>
      </div>

      <div className="divider" />

      <div className="field">
        <span>{t('algo.similarMatch')}</span>

        <label className="check">
          <input
            type="checkbox"
            checked={options.detectPerceptual}
            onChange={(e) => patch({ detectPerceptual: e.target.checked })}
            disabled={disabled}
          />
          <span>
            {t('algo.perceptual')}
            <small>{t('algo.perceptualHint')}</small>
          </span>
        </label>
        {options.detectPerceptual && (
          <div className="slider-row">
            <span className="slider-label">{t('algo.threshold')}</span>
            <input
              type="range"
              min={70}
              max={100}
              value={Math.round(options.perceptualThreshold * 100)}
              onChange={(e) => patch({ perceptualThreshold: Number(e.target.value) / 100 })}
              disabled={disabled}
            />
            <span className="slider-value">{(options.perceptualThreshold * 100).toFixed(0)}%</span>
          </div>
        )}
        {options.detectPerceptual && (
          <small className="hint">{t('algo.thresholdHint')}</small>
        )}

        <label className="check" style={{ marginTop: 6 }}>
          <input
            type="checkbox"
            checked={options.audioFingerprint}
            onChange={(e) => patch({ audioFingerprint: e.target.checked })}
            disabled={disabled}
          />
          <span>
            {t('algo.audio')}
            <small>{t('algo.audioHint')}</small>
          </span>
        </label>
        {options.audioFingerprint && (
          <div className="slider-row">
            <span className="slider-label">{t('algo.threshold')}</span>
            <input
              type="range"
              min={70}
              max={100}
              value={Math.round(options.audioThreshold * 100)}
              onChange={(e) => patch({ audioThreshold: Number(e.target.value) / 100 })}
              disabled={disabled}
            />
            <span className="slider-value">{(options.audioThreshold * 100).toFixed(0)}%</span>
          </div>
        )}
        {options.audioFingerprint && (
          <small className="hint">{t('algo.audioHint2')}</small>
        )}

        <label className="check" style={{ marginTop: 6 }}>
          <input
            type="checkbox"
            checked={options.scanArchives}
            onChange={(e) => patch({ scanArchives: e.target.checked })}
            disabled={disabled}
          />
          <span>
            {t('algo.archives')}
            <small>{t('algo.archivesHint')}</small>
          </span>
        </label>
      </div>
    </div>
  );
}
