import { humanBytes } from '../../format';
import { useI18n } from '../../i18n';
import type { ScanFilter, ScanOptions } from '../../types';
import { Icon } from '../Icon';

interface Props {
  options: ScanOptions;
  patchFilter: (value: Partial<ScanFilter>) => void;
  disabled: boolean;
}

/** `YYYY-MM-DD` → Unix 秒；空串返回 null。 */
function toUnixSeconds(value: string, endOfDay = false): number | null {
  if (!value) return null;
  const ms = new Date(`${value}T${endOfDay ? '23:59:59' : '00:00:00'}`).getTime();
  return Number.isFinite(ms) ? Math.floor(ms / 1000) : null;
}

function fromUnixSeconds(value: number | null): string {
  if (!value) return '';
  const d = new Date(value * 1000);
  if (Number.isNaN(d.getTime())) return '';
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** 逗号分隔的扩展名输入 → 归一化数组（去点、转小写、去空）。 */
function parseExts(value: string): string[] {
  return value
    .split(',')
    .map((s) => s.trim().replace(/^\./, '').toLowerCase())
    .filter(Boolean);
}

export function FilterSettings({ options, patchFilter, disabled }: Props) {
  const { t } = useI18n();
  const f = options.filter;

  return (
    <div className="settings-form">
      <div className="form-grid">
        <label className="field">
          <span>{t('filter.onlyExt')}</span>
          <input
            type="text"
            value={f.includeExtensions.join(', ')}
            onChange={(e) => patchFilter({ includeExtensions: parseExts(e.target.value) })}
            placeholder={t('filter.onlyExtPh')}
            disabled={disabled}
          />
          <small className="hint">{t('filter.onlyExtHint')}</small>
        </label>

        <label className="field">
          <span>{t('filter.excludeExt')}</span>
          <input
            type="text"
            value={f.excludeExtensions.join(', ')}
            onChange={(e) => patchFilter({ excludeExtensions: parseExts(e.target.value) })}
            placeholder={t('filter.excludeExtPh')}
            disabled={disabled}
          />
          <small className="hint">{t('filter.excludeExtHint')}</small>
        </label>
      </div>

      <div className="divider" />

      <div className="form-grid">
        <label className="field">
          <span>{t('filter.minSize')}</span>
          <div className="row">
            <input
              type="number"
              min={0}
              step={1024}
              value={f.minSize}
              onChange={(e) => patchFilter({ minSize: Math.max(0, Number(e.target.value) || 0) })}
              disabled={disabled}
            />
            <span className="hint">
              {f.minSize > 0 ? t('filter.sizeVal', { size: humanBytes(f.minSize) }) : t('common.unlimited')}
            </span>
          </div>
          <small className="hint">{t('filter.minSizeHint')}</small>
        </label>

        <label className="field">
          <span>{t('filter.maxSize')}</span>
          <div className="row">
            <input
              type="number"
              min={0}
              step={1024}
              value={f.maxSize ?? 0}
              onChange={(e) => {
                const v = Math.max(0, Number(e.target.value) || 0);
                patchFilter({ maxSize: v === 0 ? null : v });
              }}
              disabled={disabled}
            />
            <span className="hint">
              {f.maxSize ? t('filter.sizeVal', { size: humanBytes(f.maxSize) }) : t('filter.maxSizeZero')}
            </span>
          </div>
          <small className="hint">{t('filter.maxSizeHint')}</small>
        </label>
      </div>

      <div className="form-grid">
        <label className="field">
          <span>{t('filter.modAfter')}</span>
          <input
            type="date"
            value={fromUnixSeconds(f.modifiedAfter)}
            onChange={(e) => patchFilter({ modifiedAfter: toUnixSeconds(e.target.value) })}
            disabled={disabled}
          />
        </label>

        <label className="field">
          <span>{t('filter.modBefore')}</span>
          <input
            type="date"
            value={fromUnixSeconds(f.modifiedBefore)}
            onChange={(e) => patchFilter({ modifiedBefore: toUnixSeconds(e.target.value, true) })}
            disabled={disabled}
          />
          <small className="hint">{t('filter.modHint')}</small>
        </label>
      </div>

      <div className="divider" />

      <label className="field">
        <span>{t('filter.glob')}</span>
        <input
          type="text"
          value={f.excludeGlobs.join(', ')}
          onChange={(e) =>
            patchFilter({
              excludeGlobs: e.target.value
                .split(',')
                .map((s) => s.trim())
                .filter(Boolean),
            })
          }
          placeholder={t('filter.globPh')}
          disabled={disabled}
        />
        <small className="hint">{t('filter.globHint')}</small>
      </label>

      <div className="divider" />

      <div className="field">
        <span>{t('filter.attrTitle')}</span>
        <label className="check">
          <input
            type="checkbox"
            checked={f.includeHidden}
            onChange={(e) => patchFilter({ includeHidden: e.target.checked })}
            disabled={disabled}
          />
          <span>
            {t('filter.includeHidden')}
            <small>{t('filter.includeHiddenHint')}</small>
          </span>
        </label>
        <label className="check">
          <input
            type="checkbox"
            checked={f.includeSystem}
            onChange={(e) => patchFilter({ includeSystem: e.target.checked })}
            disabled={disabled}
          />
          <span>
            {t('filter.includeSystem')}
            <small>{t('filter.includeSystemHint')}</small>
          </span>
        </label>
        <label className="check">
          <input
            type="checkbox"
            checked={f.followSymlinks}
            onChange={(e) => patchFilter({ followSymlinks: e.target.checked })}
            disabled={disabled}
          />
          <span>
            {t('filter.followSym')}
            <small>{t('filter.followSymHint')}</small>
          </span>
        </label>
      </div>

      {f.excludeGlobs.length > 0 && (
        <div className="alert info" style={{ marginTop: 12 }}>
          <div className="row" style={{ gap: 6, marginBottom: 6 }}>
            <Icon name="filter" size={13} />
            <span>{t('filter.activeGlobs', { n: f.excludeGlobs.length })}</span>
          </div>
          <div className="chips">
            {f.excludeGlobs.map((g) => (
              <span className="tag" key={g}>
                {g}
              </span>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
