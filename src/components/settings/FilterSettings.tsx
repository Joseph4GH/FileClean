import { humanBytes } from '../../format';
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
  const f = options.filter;

  return (
    <div className="settings-form">
      <div className="form-grid">
        <label className="field">
          <span>仅扫描这些扩展名</span>
          <input
            type="text"
            value={f.includeExtensions.join(', ')}
            onChange={(e) => patchFilter({ includeExtensions: parseExts(e.target.value) })}
            placeholder="jpg, png, mp4"
            disabled={disabled}
          />
          <small className="hint">逗号分隔，留空表示不限制类型</small>
        </label>

        <label className="field">
          <span>排除这些扩展名</span>
          <input
            type="text"
            value={f.excludeExtensions.join(', ')}
            onChange={(e) => patchFilter({ excludeExtensions: parseExts(e.target.value) })}
            placeholder="iso, dmg"
            disabled={disabled}
          />
          <small className="hint">白名单与黑名单同时存在时，黑名单优先</small>
        </label>
      </div>

      <div className="divider" />

      <div className="form-grid">
        <label className="field">
          <span>最小体积</span>
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
              {f.minSize > 0 ? humanBytes(f.minSize) : '不限制'}（字节）
            </span>
          </div>
          <small className="hint">小于该体积的文件直接跳过，用于滤掉图标、缩略图等小文件</small>
        </label>

        <label className="field">
          <span>最大体积</span>
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
              {f.maxSize ? humanBytes(f.maxSize) : '不限制'}（0 = 不限）
            </span>
          </div>
          <small className="hint">体积预筛的第一道闸门，超出范围的文件不会参与哈希</small>
        </label>
      </div>

      <div className="form-grid">
        <label className="field">
          <span>修改时间晚于</span>
          <input
            type="date"
            value={fromUnixSeconds(f.modifiedAfter)}
            onChange={(e) => patchFilter({ modifiedAfter: toUnixSeconds(e.target.value) })}
            disabled={disabled}
          />
        </label>

        <label className="field">
          <span>修改时间早于</span>
          <input
            type="date"
            value={fromUnixSeconds(f.modifiedBefore)}
            onChange={(e) => patchFilter({ modifiedBefore: toUnixSeconds(e.target.value, true) })}
            disabled={disabled}
          />
          <small className="hint">含当日 23:59:59</small>
        </label>
      </div>

      <div className="divider" />

      <label className="field">
        <span>排除规则（glob 通配，逗号分隔）</span>
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
          placeholder="**/node_modules/**, **/Cache/**"
          disabled={disabled}
        />
        <small className="hint">
          匹配整条路径，命中即整个目录被剪枝（不再向下遍历），比扩展名过滤更省时间
        </small>
      </label>

      <div className="divider" />

      <div className="field">
        <span>文件属性</span>
        <label className="check">
          <input
            type="checkbox"
            checked={f.includeHidden}
            onChange={(e) => patchFilter({ includeHidden: e.target.checked })}
            disabled={disabled}
          />
          <span>
            包含隐藏文件
            <small>以「.」开头的文件与隐藏属性的文件</small>
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
            包含系统文件
            <small>Thumbs.db / .DS_Store / 备份临时文件，默认跳过</small>
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
            跟随符号链接
            <small>默认关闭：开启后可能因链接成环而重复计数</small>
          </span>
        </label>
      </div>

      {f.excludeGlobs.length > 0 && (
        <div className="alert info" style={{ marginTop: 12 }}>
          <div className="row" style={{ gap: 6, marginBottom: 6 }}>
            <Icon name="filter" size={13} />
            <span>当前生效的排除规则（{f.excludeGlobs.length} 条）</span>
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
