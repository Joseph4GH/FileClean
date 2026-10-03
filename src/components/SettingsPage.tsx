import { useMemo, useState } from 'react';
import type { ReactNode } from 'react';

import type { AutoMarkConfig, ScanFilter, ScanOptions, SystemInfo } from '../types';
import { Icon } from './Icon';
import type { IconName } from './Icon';
import { AlgorithmSettings } from './settings/AlgorithmSettings';
import { FilterSettings } from './settings/FilterSettings';
import { MarkingSettings } from './settings/MarkingSettings';
import { PerformanceSettings } from './settings/PerformanceSettings';
import { SafetySettings } from './settings/SafetySettings';

export type SettingsSection =
  | 'filter'
  | 'algorithm'
  | 'safety'
  | 'performance'
  | 'marking'
  | 'session';

interface NavItem {
  key: SettingsSection;
  label: string;
  icon: IconName;
  desc: string;
}

/** 导航按「规则 → 安全性能 → 策略数据」三段组织，每段两个入口，避免一长条平铺。 */
const NAV: Array<{ title: string; items: NavItem[] }> = [
  {
    title: '扫描规则',
    items: [
      { key: 'filter', label: '过滤条件', icon: 'filter', desc: '扩展名、体积、日期与排除规则' },
      { key: 'algorithm', label: '匹配算法', icon: 'sliders', desc: '哈希算法与相似匹配开关' },
    ],
  },
  {
    title: '安全与性能',
    items: [
      { key: 'safety', label: '安全与保护', icon: 'shield', desc: '系统保护锁与引用完整性检查' },
      { key: 'performance', label: '性能与缓存', icon: 'gauge', desc: '并发线程、分组门槛与增量缓存' },
    ],
  },
  {
    title: '策略与数据',
    items: [
      { key: 'marking', label: '智能保留策略', icon: 'check', desc: '规则链、保底份数与受保护文件' },
      { key: 'session', label: '会话与导入导出', icon: 'save', desc: '保存、载入与导入导出会话' },
    ],
  },
];

const TITLES: Record<SettingsSection, string> = {
  filter: '过滤条件',
  algorithm: '匹配算法',
  safety: '安全与保护',
  performance: '性能与缓存',
  marking: '智能保留策略',
  session: '会话与导入导出',
};

interface Props {
  options: ScanOptions;
  onOptions: (next: ScanOptions) => void;
  markConfig: AutoMarkConfig;
  onMarkConfig: (next: AutoMarkConfig) => void;
  systemInfo: SystemInfo | null;
  running: boolean;
  onPickSaveDir: () => void;
  onClearCache: () => void;
  /** 会话面板由 App 注入，设置页不反向依赖应用状态。 */
  sessionSlot: ReactNode;
}

export function SettingsPage({
  options,
  onOptions,
  markConfig,
  onMarkConfig,
  systemInfo,
  running,
  onPickSaveDir,
  onClearCache,
  sessionSlot,
}: Props) {
  const [active, setActive] = useState<SettingsSection>('filter');

  const patch = (value: Partial<ScanOptions>) => onOptions({ ...options, ...value });
  const patchFilter = (value: Partial<ScanFilter>) =>
    onOptions({ ...options, filter: { ...options.filter, ...value } });

  const activeItem = useMemo(
    () => NAV.flatMap((g) => g.items).find((i) => i.key === active)!,
    [active],
  );

  return (
    <div className="settings">
      <nav className="settings-nav">
        {NAV.map((group) => (
          <div className="nav-group" key={group.title}>
            <div className="nav-group-title">{group.title}</div>
            {group.items.map((item) => (
              <button
                key={item.key}
                className={`nav-item${active === item.key ? ' active' : ''}`}
                onClick={() => setActive(item.key)}
              >
                <Icon name={item.icon} size={15} />
                <span className="nav-text">
                  <span className="nav-label">{item.label}</span>
                  <small>{item.desc}</small>
                </span>
              </button>
            ))}
          </div>
        ))}

        {running && (
          <div className="alert warn" style={{ margin: '12px 4px 0' }}>
            扫描进行中，设置暂不可修改。
          </div>
        )}
      </nav>

      <section className="settings-pane">
        <header className="pane-head">
          <div>
            <h2>
              <Icon name={activeItem.icon} size={16} /> {TITLES[active]}
            </h2>
            <p className="hint">{activeItem.desc}</p>
          </div>
        </header>

        <div className="pane-body">
          {active === 'filter' && (
            <FilterSettings options={options} patchFilter={patchFilter} disabled={running} />
          )}
          {active === 'algorithm' && (
            <AlgorithmSettings options={options} patch={patch} disabled={running} />
          )}
          {active === 'safety' && (
            <SafetySettings
              options={options}
              patch={patch}
              disabled={running}
              systemInfo={systemInfo}
            />
          )}
          {active === 'performance' && (
            <PerformanceSettings
              options={options}
              patch={patch}
              disabled={running}
              systemInfo={systemInfo}
              onPickSaveDir={onPickSaveDir}
              onClearCache={onClearCache}
            />
          )}
          {active === 'marking' && (
            <MarkingSettings markConfig={markConfig} onMarkConfig={onMarkConfig} />
          )}
          {active === 'session' && sessionSlot}
        </div>
      </section>
    </div>
  );
}
