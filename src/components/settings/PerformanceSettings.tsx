import { num } from '../../format';
import { useI18n } from '../../i18n';
import type { ScanOptions, SystemInfo } from '../../types';
import { Icon } from '../Icon';

interface Props {
  options: ScanOptions;
  patch: (value: Partial<ScanOptions>) => void;
  disabled: boolean;
  systemInfo: SystemInfo | null;
  onPickSaveDir: () => void;
  onClearCache: () => void;
}

export function PerformanceSettings({
  options,
  patch,
  disabled,
  systemInfo,
  onPickSaveDir,
  onClearCache,
}: Props) {
  const { t } = useI18n();

  return (
    <div className="settings-form">
      <div className="form-grid">
        <label className="field">
          <span>{t('perf.threads')}</span>
          <div className="row">
            <input
              type="number"
              min={0}
              max={256}
              value={options.threads}
              onChange={(e) => patch({ threads: Math.max(0, Number(e.target.value) || 0) })}
              disabled={disabled}
            />
            <span className="hint">
              {t('perf.threadsAuto', { n: systemInfo?.effectiveThreads ?? '—' })}
            </span>
          </div>
          <small className="hint">{t('perf.threadsHint')}</small>
        </label>

        <label className="field">
          <span>{t('perf.minGroup')}</span>
          <div className="row">
            <input
              type="number"
              min={2}
              max={100}
              value={options.minGroupSize}
              onChange={(e) => patch({ minGroupSize: Math.max(2, Number(e.target.value) || 2) })}
              disabled={disabled}
            />
            <span className="hint">{t('common.unit')}</span>
          </div>
          <small className="hint">{t('perf.minGroupHint')}</small>
        </label>
      </div>

      <div className="divider" />

      <div className="card">
        <label className="check">
          <input
            type="checkbox"
            checked={options.useCache}
            onChange={(e) => patch({ useCache: e.target.checked })}
            disabled={disabled}
          />
          <span>
            {t('perf.cache')}
            <small>{t('perf.cacheHint', { n: num(systemInfo?.cacheEntries ?? 0) })}</small>
          </span>
        </label>
        <small className="hint" style={{ paddingLeft: 21 }}>
          {t('perf.cacheDetail')}
        </small>

        <div className="row" style={{ marginTop: 10 }}>
          <button className="btn sm" onClick={onClearCache} disabled={disabled}>
            <Icon name="trash" size={13} /> {t('perf.clearCache')}
          </button>
          <button className="btn sm" onClick={onPickSaveDir}>
            <Icon name="export" size={13} /> {t('perf.openExport')}
          </button>
        </div>
      </div>

      <div className="divider" />

      <div className="field">
        <span>{t('perf.env')}</span>
        <div className="kv-block">
          <div className="kv">
            <span className="k">{t('kv.platform')}</span>
            <span className="v">
              {systemInfo ? `${systemInfo.platform} / ${systemInfo.arch}` : '—'}
            </span>
            <span className="k">{t('kv.cores')}</span>
            <span className="v">{systemInfo ? num(systemInfo.logicalCores) : '—'}</span>
            <span className="k">{t('kv.cacheEntries')}</span>
            <span className="v">{systemInfo ? num(systemInfo.cacheEntries) : '—'}</span>
            <span className="k">{t('kv.exportDir')}</span>
            <span className="v mono break">{systemInfo?.exportDir ?? '—'}</span>
          </div>
        </div>
      </div>
    </div>
  );
}
