import { useMemo, useState } from 'react';
import type { ReactNode } from 'react';

import type { AutoMarkConfig, ScanFilter, ScanOptions, SystemInfo } from '../types';
import { Icon } from './Icon';
import type { IconName } from './Icon';
import { useI18n, type DictKey } from '../i18n';
import { AlgorithmSettings } from './settings/AlgorithmSettings';
import { FilterSettings } from './settings/FilterSettings';
import { MarkingSettings } from './settings/MarkingSettings';
import { PerformanceSettings } from './settings/PerformanceSettings';
import { SafetySettings } from './settings/SafetySettings';
import { AppearanceSettings } from './settings/AppearanceSettings';

export type SettingsSection =
  | 'filter'
  | 'algorithm'
  | 'safety'
  | 'performance'
  | 'marking'
  | 'session'
  | 'appearance';

interface NavItem {
  key: SettingsSection;
  labelKey: DictKey;
  icon: IconName;
  descKey: DictKey;
}

/** 导航分四段组织：扫描规则 / 安全性能 / 策略数据 / 外观。 */
const NAV_GROUPS: Array<{ titleKey: DictKey; items: NavItem[] }> = [
  {
    titleKey: 'navGrp.rules',
    items: [
      { key: 'filter', labelKey: 'nav.filter', icon: 'filter', descKey: 'nav.filterDesc' },
      { key: 'algorithm', labelKey: 'nav.algorithm', icon: 'sliders', descKey: 'nav.algorithmDesc' },
    ],
  },
  {
    titleKey: 'navGrp.safetyPerf',
    items: [
      { key: 'safety', labelKey: 'nav.safety', icon: 'shield', descKey: 'nav.safetyDesc' },
      {
        key: 'performance',
        labelKey: 'nav.performance',
        icon: 'gauge',
        descKey: 'nav.performanceDesc',
      },
    ],
  },
  {
    titleKey: 'navGrp.policyData',
    items: [
      { key: 'marking', labelKey: 'nav.marking', icon: 'check', descKey: 'nav.markingDesc' },
      { key: 'session', labelKey: 'nav.session', icon: 'save', descKey: 'nav.sessionDesc' },
    ],
  },
  {
    titleKey: 'nav.appearance',
    items: [
      { key: 'appearance', labelKey: 'nav.appearance', icon: 'palette', descKey: 'nav.appearanceDesc' },
    ],
  },
];

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
  const { t } = useI18n();
  const [active, setActive] = useState<SettingsSection>('filter');

  const patch = (value: Partial<ScanOptions>) => onOptions({ ...options, ...value });
  const patchFilter = (value: Partial<ScanFilter>) =>
    onOptions({ ...options, filter: { ...options.filter, ...value } });

  const activeItem = useMemo(
    () => NAV_GROUPS.flatMap((g) => g.items).find((i) => i.key === active)!,
    [active],
  );

  return (
    <div className="settings">
      <nav className="settings-nav">
        {NAV_GROUPS.map((group) => (
          <div className="nav-group" key={group.titleKey}>
            <div className="nav-group-title">{t(group.titleKey)}</div>
            {group.items.map((item) => (
              <button
                key={item.key}
                className={`nav-item${active === item.key ? ' active' : ''}`}
                onClick={() => setActive(item.key)}
              >
                <Icon name={item.icon} size={15} />
                <span className="nav-text">
                  <span className="nav-label">{t(item.labelKey)}</span>
                  <small>{t(item.descKey)}</small>
                </span>
              </button>
            ))}
          </div>
        ))}

        {running && (
          <div className="alert warn" style={{ margin: '12px 4px 0' }}>
            {t('settings.locked')}
          </div>
        )}
      </nav>

      <section className="settings-pane">
        <header className="pane-head">
          <div>
            <h2>
              <Icon name={activeItem.icon} size={16} /> {t(activeItem.labelKey)}
            </h2>
            <p className="hint">{t(activeItem.descKey)}</p>
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
            <SafetySettings options={options} patch={patch} disabled={running} systemInfo={systemInfo} />
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
          {active === 'appearance' && <AppearanceSettings />}
        </div>
      </section>
    </div>
  );
}
