import { useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';

import { ALGORITHM_OPTIONS, baseName, humanBytes, num } from '../format';
import type { AutoMarkConfig, ScanOptions, SystemInfo } from '../types';
import { Icon } from './Icon';
import type { IconName } from './Icon';

interface Props {
  options: ScanOptions;
  onOptions: (next: ScanOptions) => void;
  markConfig: AutoMarkConfig;
  running: boolean;
  systemInfo: SystemInfo | null;
  onStart: () => void;
  onCancel: () => void;
  /** 跳到「扫描规则设置」页。配置项都在那边，这里只给一个入口和只读摘要。 */
  onOpenSettings: () => void;
}

const PRESETS: Array<{ key: string; label: string; icon: IconName; patch: Partial<ScanOptions> }> = [
  {
    key: 'photo',
    label: '照片去重',
    icon: 'image',
    patch: {
      algorithm: 'blake3',
      minGroupSize: 2,
      detectPerceptual: true,
      perceptualThreshold: 0.92,
      audioFingerprint: false,
      scanArchives: false,
      rawJpegPairing: true,
    },
  },
  {
    key: 'fast',
    label: '快速扫磁盘',
    icon: 'disk',
    patch: {
      algorithm: 'xxhash3',
      minGroupSize: 2,
      detectPerceptual: false,
      audioFingerprint: false,
      scanArchives: false,
      rawJpegPairing: false,
    },
  },
  {
    key: 'music',
    label: '音乐去重',
    icon: 'audio',
    patch: {
      algorithm: 'blake3',
      minGroupSize: 2,
      detectPerceptual: false,
      audioFingerprint: true,
      audioThreshold: 0.92,
      scanArchives: false,
      rawJpegPairing: false,
    },
  },
  {
    key: 'deep',
    label: '深度审计',
    icon: 'shield',
    patch: {
      algorithm: 'sha256',
      minGroupSize: 2,
      detectPerceptual: true,
      audioFingerprint: true,
      scanArchives: true,
      rawJpegPairing: true,
      checkReferences: true,
    },
  },
];

export function SetupPanel({
  options,
  onOptions,
  markConfig,
  running,
  systemInfo,
  onStart,
  onCancel,
  onOpenSettings,
}: Props) {
  const [busy, setBusy] = useState(false);

  const patch = (value: Partial<ScanOptions>) => onOptions({ ...options, ...value });

  const addRoots = async () => {
    setBusy(true);
    try {
      const picked = await open({ directory: true, multiple: true, title: '选择要扫描的目录' });
      if (!picked) return;
      const list = Array.isArray(picked) ? picked : [picked];
      patch({ roots: Array.from(new Set([...options.roots, ...list])) });
    } finally {
      setBusy(false);
    }
  };

  const removeRoot = (root: string) => patch({ roots: options.roots.filter((r) => r !== root) });

  const applyPreset = (preset: (typeof PRESETS)[number]) => {
    const next: ScanOptions = { ...options, ...preset.patch };
    next.filter = { ...options.filter };
    if (preset.key === 'photo' || preset.key === 'music') {
      next.filter.includeExtensions = [];
      next.filter.minSize = 1;
    }
    if (preset.key === 'deep') next.filter.includeHidden = true;
    onOptions(next);
  };

  // ---- 只读摘要：把「会影响扫描结果的规则」压成一眼能看完的几行 ----
  const algoLabel =
    ALGORITHM_OPTIONS.find((o) => o.value === options.algorithm)?.label ?? options.algorithm;

  const features: Array<{ label: string; tone: 'ok' | 'warn' | '' }> = [];
  if (options.rawJpegPairing) features.push({ label: 'RAW/JPEG 配对', tone: '' });
  if (options.detectPerceptual)
    features.push({ label: `图片相似 ${(options.perceptualThreshold * 100).toFixed(0)}%`, tone: 'warn' });
  if (options.audioFingerprint)
    features.push({ label: `音频指纹 ${(options.audioThreshold * 100).toFixed(0)}%`, tone: 'warn' });
  if (options.scanArchives) features.push({ label: '压缩包内检索', tone: '' });
  if (options.checkReferences) features.push({ label: '引用完整性', tone: 'ok' });
  if (options.protectSystemDirs) features.push({ label: '系统保护锁', tone: 'ok' });
  if (!options.protectSystemDirs) features.push({ label: '保护锁已关闭', tone: 'warn' });

  const enabledRules = markConfig.rules.filter((r) => r.enabled).length;

  const f = options.filter;
  const filterParts: string[] = [];
  if (f.includeExtensions.length > 0) filterParts.push(`白名单 ${f.includeExtensions.length} 类`);
  if (f.excludeExtensions.length > 0) filterParts.push(`排除 ${f.excludeExtensions.length} 类`);
  if (f.excludeGlobs.length > 0) filterParts.push(`${f.excludeGlobs.length} 条 glob`);
  if (f.minSize > 0) filterParts.push(`≥ ${humanBytes(f.minSize)}`);
  if (f.maxSize) filterParts.push(`≤ ${humanBytes(f.maxSize)}`);
  if (f.modifiedAfter || f.modifiedBefore) filterParts.push('按日期限定');
  if (f.includeHidden) filterParts.push('含隐藏文件');
  if (f.includeSystem) filterParts.push('含系统文件');
  const filterSummary = filterParts.length > 0 ? filterParts.join(' · ') : '无限制（全部文件）';

  return (
    <div className="sidebar">
      <div className="section">
        <h3>
          <Icon name="folder" size={14} /> 扫描目录
          <span className="count">（{options.roots.length}）</span>
        </h3>
        <div className="btn-row" style={{ marginBottom: 8 }}>
          <button className="btn" onClick={addRoots} disabled={busy || running}>
            <Icon name="plus" size={13} /> 添加目录
          </button>
          <button
            className="btn"
            onClick={() => patch({ roots: [] })}
            disabled={running || options.roots.length === 0}
          >
            清空
          </button>
        </div>

        {options.roots.length === 0 ? (
          <div className="hint">还没选择目录。可以添加多个目录，也支持网络路径与外接磁盘。</div>
        ) : (
          <div>
            {options.roots.map((root) => (
              <div className="session-item" key={root}>
                <div className="info">
                  <div>{baseName(root) || root}</div>
                  <div title={root}>{root}</div>
                </div>
                <button
                  className="btn ghost sm"
                  onClick={() => removeRoot(root)}
                  disabled={running}
                  title="移除"
                >
                  <Icon name="minus" size={13} />
                </button>
              </div>
            ))}
          </div>
        )}
      </div>

      <div className="section">
        <h3>
          <Icon name="clock" size={14} /> 快捷场景
        </h3>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 6 }}>
          {PRESETS.map((p) => (
            <button key={p.key} className="btn sm" onClick={() => applyPreset(p)} disabled={running}>
              <Icon name={p.icon} size={13} /> {p.label}
            </button>
          ))}
        </div>
        <div className="hint" style={{ marginTop: 6 }}>
          场景只覆盖匹配相关的开关，过滤条件与保留策略保持不变
        </div>
      </div>

      <div className="section">
        <h3>
          <Icon name="sliders" size={14} /> 当前规则
        </h3>
        <div className="card">
          <div className="kv tight">
            <span className="k">哈希算法</span>
            <span className="v">{algoLabel}</span>
            <span className="k">过滤条件</span>
            <span className="v">{filterSummary}</span>
            <span className="k">分组门槛</span>
            <span className="v">≥ {options.minGroupSize} 份</span>
            <span className="k">并发</span>
            <span className="v">
              {options.threads === 0
                ? `自动（${systemInfo?.effectiveThreads ?? '—'} 线程）`
                : `${options.threads} 线程`}
            </span>
            <span className="k">增量缓存</span>
            <span className="v">
              {options.useCache ? `启用（${num(systemInfo?.cacheEntries ?? 0)} 条）` : '已关闭'}
            </span>
            <span className="k">保留策略</span>
            <span className="v">
              {enabledRules} 条规则 · 每组留 {markConfig.keepPerGroup} 份
            </span>
          </div>

          {features.length > 0 && (
            <div className="row wrap" style={{ marginTop: 9, gap: 5 }}>
              {features.map((feat) => (
                <span className={`tag${feat.tone ? ` ${feat.tone}` : ''}`} key={feat.label}>
                  {feat.label}
                </span>
              ))}
            </div>
          )}

          <button className="btn block sm" style={{ marginTop: 10 }} onClick={onOpenSettings}>
            <Icon name="settings" size={13} /> 调整扫描规则
          </button>
        </div>
      </div>

      <div
        style={{
          position: 'sticky',
          bottom: -14,
          background: 'var(--panel)',
          paddingTop: 10,
          paddingBottom: 4,
          marginTop: 4,
        }}
      >
        {running ? (
          <button className="btn danger block" onClick={onCancel}>
            <Icon name="cross" size={14} /> 取消扫描
          </button>
        ) : (
          <button
            className="btn primary block"
            onClick={onStart}
            disabled={options.roots.length === 0}
            title={options.roots.length === 0 ? '请先添加扫描目录' : '开始扫描'}
          >
            <Icon name="search" size={14} /> 开始扫描
          </button>
        )}
      </div>
    </div>
  );
}
