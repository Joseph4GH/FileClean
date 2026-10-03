import { num } from '../../format';
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
  return (
    <div className="settings-form">
      <div className="form-grid">
        <label className="field">
          <span>并发线程数</span>
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
              0 = 自动（当前 {systemInfo?.effectiveThreads ?? '—'} 线程）
            </span>
          </div>
          <small className="hint">
            自动值取逻辑核心数；机械盘上盲目调高反而会因寻道抖动而变慢
          </small>
        </label>

        <label className="field">
          <span>分组最少文件数</span>
          <div className="row">
            <input
              type="number"
              min={2}
              max={100}
              value={options.minGroupSize}
              onChange={(e) => patch({ minGroupSize: Math.max(2, Number(e.target.value) || 2) })}
              disabled={disabled}
            />
            <span className="hint">份</span>
          </div>
          <small className="hint">设为 3 可以只看「三胞胎以上」，过滤掉零散的两两重复</small>
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
            使用增量扫描缓存
            <small>
              已缓存 {num(systemInfo?.cacheEntries ?? 0)} 条指纹；只重算新增或已修改的文件
            </small>
          </span>
        </label>
        <small className="hint" style={{ paddingLeft: 21 }}>
          缓存以「路径 + 体积 + 修改时间 + inode」四元组判定是否失效，改过系统时间或伪造 mtime
          都不会造成漏检。
        </small>

        <div className="row" style={{ marginTop: 10 }}>
          <button className="btn sm" onClick={onClearCache} disabled={disabled}>
            <Icon name="trash" size={13} /> 清空增量缓存
          </button>
          <button className="btn sm" onClick={onPickSaveDir}>
            <Icon name="export" size={13} /> 打开导出目录
          </button>
        </div>
      </div>

      <div className="divider" />

      <div className="field">
        <span>运行环境</span>
        <div className="kv-block">
          <div className="kv">
            <span className="k">平台</span>
            <span className="v">
              {systemInfo ? `${systemInfo.platform} / ${systemInfo.arch}` : '—'}
            </span>
            <span className="k">逻辑核心</span>
            <span className="v">{systemInfo ? num(systemInfo.logicalCores) : '—'}</span>
            <span className="k">缓存条目</span>
            <span className="v">{systemInfo ? num(systemInfo.cacheEntries) : '—'}</span>
            <span className="k">导出目录</span>
            <span className="v mono break">{systemInfo?.exportDir ?? '—'}</span>
          </div>
        </div>
      </div>
    </div>
  );
}
