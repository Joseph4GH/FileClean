import { useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';

import { baseName, humanBytes, num } from '../format';
import { algorithmOptions } from '../i18n/meta';
import { useI18n, type DictKey } from '../i18n';
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

const PRESETS: Array<{ key: string; icon: IconName; patch: Partial<ScanOptions> }> = [
  {
    key: 'photo',
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

const PRESET_LABEL_KEYS: Record<string, DictKey> = {
  photo: 'preset.photo',
  fast: 'preset.fast',
  music: 'preset.music',
  deep: 'preset.deep',
};

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
  const { t } = useI18n();
  const [busy, setBusy] = useState(false);

  const patch = (value: Partial<ScanOptions>) => onOptions({ ...options, ...value });

  const addRoots = async () => {
    setBusy(true);
    try {
      const picked = await open({ directory: true, multiple: true, title: t('setup.pickDirTitle') });
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
    algorithmOptions().find((o) => o.value === options.algorithm)?.label ?? options.algorithm;

  const features: Array<{ label: string; tone: 'ok' | 'warn' | '' }> = [];
  if (options.rawJpegPairing) features.push({ label: t('feature.rawJpeg'), tone: '' });
  if (options.detectPerceptual)
    features.push({ label: `${t('feature.perceptual')} ${(options.perceptualThreshold * 100).toFixed(0)}%`, tone: 'warn' });
  if (options.audioFingerprint)
    features.push({ label: `${t('feature.audio')} ${(options.audioThreshold * 100).toFixed(0)}%`, tone: 'warn' });
  if (options.scanArchives) features.push({ label: t('feature.archives'), tone: '' });
  if (options.checkReferences) features.push({ label: t('feature.refs'), tone: 'ok' });
  if (options.protectSystemDirs) features.push({ label: t('feature.protectOn'), tone: 'ok' });
  if (!options.protectSystemDirs) features.push({ label: t('feature.protectOff'), tone: 'warn' });

  const enabledRules = markConfig.rules.filter((r) => r.enabled).length;

  const f = options.filter;
  const filterParts: string[] = [];
  if (f.includeExtensions.length > 0)
    filterParts.push(t('summary.allowExt', { n: f.includeExtensions.length }));
  if (f.excludeExtensions.length > 0)
    filterParts.push(t('summary.excludeExt', { n: f.excludeExtensions.length }));
  if (f.excludeGlobs.length > 0) filterParts.push(t('summary.globs', { n: f.excludeGlobs.length }));
  if (f.minSize > 0) filterParts.push(t('summary.minSize', { size: humanBytes(f.minSize) }));
  if (f.maxSize) filterParts.push(t('summary.maxSize', { size: humanBytes(f.maxSize) }));
  if (f.modifiedAfter || f.modifiedBefore) filterParts.push(t('summary.byDate'));
  if (f.includeHidden) filterParts.push(t('summary.hidden'));
  if (f.includeSystem) filterParts.push(t('summary.system'));
  const filterSummary = filterParts.length > 0 ? filterParts.join(' · ') : t('setup.filterUnlimited');

  return (
    <div className="sidebar">
      <div className="section">
        <h3>
          <Icon name="folder" size={14} /> {t('setup.dirsTitle')}
          <span className="count">{t('setup.dirsCount', { n: options.roots.length })}</span>
        </h3>
        <div className="btn-row" style={{ marginBottom: 8 }}>
          <button className="btn" onClick={addRoots} disabled={busy || running}>
            <Icon name="plus" size={13} /> {t('setup.addDir')}
          </button>
          <button
            className="btn"
            onClick={() => patch({ roots: [] })}
            disabled={running || options.roots.length === 0}
          >
            {t('btn.clear')}
          </button>
        </div>

        {options.roots.length === 0 ? (
          <div className="hint">{t('setup.noDirsHint')}</div>
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
                  title={t('common.delete')}
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
          <Icon name="clock" size={14} /> {t('setup.presetsTitle')}
        </h3>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 6 }}>
          {PRESETS.map((p) => (
            <button key={p.key} className="btn sm" onClick={() => applyPreset(p)} disabled={running}>
              <Icon name={p.icon} size={13} /> {t(PRESET_LABEL_KEYS[p.key])}
            </button>
          ))}
        </div>
        <div className="hint" style={{ marginTop: 6 }}>
          {t('setup.presetsHint')}
        </div>
      </div>

      <div className="section">
        <h3>
          <Icon name="sliders" size={14} /> {t('setup.rulesTitle')}
        </h3>
        <div className="card">
          <div className="kv tight">
            <span className="k">{t('kv.algorithm')}</span>
            <span className="v">{algoLabel}</span>
            <span className="k">{t('kv.filter')}</span>
            <span className="v">{filterSummary}</span>
            <span className="k">{t('kv.minGroup')}</span>
            <span className="v">{t('setup.minGroupVal', { n: options.minGroupSize })}</span>
            <span className="k">{t('kv.threads')}</span>
            <span className="v">
              {options.threads === 0
                ? t('setup.threadsAuto', { n: systemInfo?.effectiveThreads ?? '—' })
                : t('setup.threadsN', { n: options.threads })}
            </span>
            <span className="k">{t('kv.cache')}</span>
            <span className="v">
              {options.useCache ? t('setup.cacheOn', { n: num(systemInfo?.cacheEntries ?? 0) }) : t('setup.cacheOff')}
            </span>
            <span className="k">{t('kv.keepPolicy')}</span>
            <span className="v">
              {t('setup.keepPolicyVal', { n: enabledRules, m: markConfig.keepPerGroup })}
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
            <Icon name="settings" size={13} /> {t('setup.adjust')}
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
            <Icon name="cross" size={14} /> {t('btn.cancel')}
          </button>
        ) : (
          <button
            className="btn primary block"
            onClick={onStart}
            disabled={options.roots.length === 0}
            title={options.roots.length === 0 ? t('btn.scanDisabledHint') : t('btn.scanTitle')}
          >
            <Icon name="search" size={14} /> {t('btn.scan')}
          </button>
        )}
      </div>
    </div>
  );
}
