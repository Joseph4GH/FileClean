import type { AutoMarkConfig } from '../../types';
import { Icon } from '../Icon';
import { RuleEditor } from '../RuleEditor';

interface Props {
  markConfig: AutoMarkConfig;
  onMarkConfig: (next: AutoMarkConfig) => void;
}

export function MarkingSettings({ markConfig, onMarkConfig }: Props) {
  const enabled = markConfig.rules.filter((r) => r.enabled).length;

  return (
    <div className="settings-form">
      <div className="alert info">
        <div className="row" style={{ gap: 6, marginBottom: 4 }}>
          <Icon name="check" size={13} />
          <span>
            当前 {markConfig.rules.length} 条规则，其中 {enabled} 条已启用
          </span>
        </div>
        <small className="hint">
          所有规则都没分出胜负时按「路径最浅」兜底，并保证每组至少留下{' '}
          {markConfig.keepPerGroup} 份。相似组默认不自动标记，需人工确认。
        </small>
      </div>

      <RuleEditor config={markConfig} onChange={onMarkConfig} />
    </div>
  );
}
