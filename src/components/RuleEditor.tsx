import { useState } from 'react';

import { ruleLabel } from '../format';
import type { AutoMarkConfig, MarkRule, MarkRuleEntry } from '../types';
import { Icon } from './Icon';

const RULE_TYPES: Array<{ type: MarkRule['type']; label: string; desc: string }> = [
  { type: 'keepInPath', label: '优先保留指定目录', desc: '路径包含关键词的文件优先保留，例如「原片」' },
  { type: 'preferExtension', label: '扩展名优先级', desc: '按顺序优先，例如 RAW > JPG' },
  { type: 'keepOldestCreated', label: '保留最早创建', desc: '更接近原始文件' },
  { type: 'keepNewest', label: '保留最新修改', desc: '适合保留最新修订版' },
  { type: 'keepOldest', label: '保留最早修改', desc: '' },
  { type: 'keepShortestPath', label: '保留路径最短', desc: '通常位于更靠上的原始目录' },
  { type: 'keepLongestName', label: '保留文件名最长', desc: '' },
  { type: 'keepShallowest', label: '保留层级最浅', desc: '' },
  { type: 'keepLargest', label: '保留体积最大', desc: '相似图中即分辨率最高的' },
  { type: 'keepMostLinked', label: '保留硬链接最多', desc: '被其它路径共用的更可能是源' },
];

export function RuleEditor({
  config,
  onChange,
}: {
  config: AutoMarkConfig;
  onChange: (next: AutoMarkConfig) => void;
}) {
  const [adding, setAdding] = useState(false);

  const update = (index: number, patch: Partial<MarkRuleEntry>) => {
    const rules = config.rules.map((entry, i) => (i === index ? { ...entry, ...patch } : entry));
    onChange({ ...config, rules });
  };

  const move = (index: number, delta: number) => {
    const target = index + delta;
    if (target < 0 || target >= config.rules.length) return;
    const rules = [...config.rules];
    [rules[index], rules[target]] = [rules[target], rules[index]];
    onChange({ ...config, rules });
  };

  const remove = (index: number) => {
    onChange({ ...config, rules: config.rules.filter((_, i) => i !== index) });
  };

  const add = (type: MarkRule['type']) => {
    let rule: MarkRule;
    if (type === 'keepInPath') rule = { type, pattern: '原片' };
    else if (type === 'preferExtension')
      rule = { type, extensions: ['cr2', 'cr3', 'nef', 'arw', 'dng'] };
    else rule = { type } as MarkRule;

    onChange({ ...config, rules: [...config.rules, { rule, enabled: true }] });
    setAdding(false);
  };

  return (
    <div>
      <div className="hint" style={{ marginBottom: 8 }}>
        规则按从上到下的优先级判定；上一条分出胜负就不再往下看。
      </div>

      {config.rules.length === 0 && <div className="hint">当前没有规则，将只按「路径最短」兜底。</div>}

      {config.rules.map((entry, index) => (
        <div className="rule-item" key={`${entry.rule.type}-${index}`}>
          <input
            type="checkbox"
            checked={entry.enabled}
            onChange={(e) => update(index, { enabled: e.target.checked })}
            title="启用 / 停用"
          />
          <span className="idx">{index + 1}</span>
          <span className="label" title={ruleLabel(entry.rule as never)}>
            {ruleLabel(entry.rule as never)}
          </span>
          <button className="btn ghost sm" onClick={() => move(index, -1)} title="上移">
            <Icon name="up" size={13} />
          </button>
          <button className="btn ghost sm" onClick={() => move(index, 1)} title="下移">
            <Icon name="down" size={13} />
          </button>
          <button className="btn ghost sm" onClick={() => remove(index)} title="删除">
            <Icon name="minus" size={13} />
          </button>
        </div>
      ))}

      {config.rules.map((entry, index) => {
        if (entry.rule.type === 'keepInPath') {
          return (
            <label className="field" key={`param-in-${index}`}>
              <span>规则 {index + 1} · 目录关键词</span>
              <input
                type="text"
                value={entry.rule.pattern}
                onChange={(e) =>
                  update(index, { rule: { type: 'keepInPath', pattern: e.target.value } })
                }
                placeholder="例如：原片 / RAW / Masters"
              />
            </label>
          );
        }
        if (entry.rule.type === 'preferExtension') {
          return (
            <label className="field" key={`param-ext-${index}`}>
              <span>规则 {index + 1} · 扩展名优先级（逗号分隔，靠前优先）</span>
              <input
                type="text"
                value={entry.rule.extensions.join(', ')}
                onChange={(e) =>
                  update(index, {
                    rule: {
                      type: 'preferExtension',
                      extensions: e.target.value
                        .split(',')
                        .map((s) => s.trim().replace(/^\./, '').toLowerCase())
                        .filter(Boolean),
                    },
                  })
                }
                placeholder="cr2, cr3, nef, arw, dng"
              />
            </label>
          );
        }
        return null;
      })}

      {adding ? (
        <div className="card" style={{ marginTop: 8 }}>
          {RULE_TYPES.map((t) => (
            <div
              key={t.type}
              className="rule-item"
              style={{ cursor: 'pointer' }}
              onClick={() => add(t.type)}
            >
              <span className="label">
                {t.label}
                {t.desc && <small className="hint" style={{ display: 'block' }}>{t.desc}</small>}
              </span>
              <Icon name="plus" size={13} />
            </div>
          ))}
          <button className="btn block sm" onClick={() => setAdding(false)}>
            取消
          </button>
        </div>
      ) : (
        <button className="btn block sm" onClick={() => setAdding(true)}>
          <Icon name="plus" size={13} /> 添加规则
        </button>
      )}

      <div className="divider" />

      <label className="field">
        <span>每组至少保留</span>
        <div className="row">
          <input
            type="number"
            min={1}
            max={20}
            value={config.keepPerGroup}
            onChange={(e) =>
              onChange({ ...config, keepPerGroup: Math.max(1, Number(e.target.value) || 1) })
            }
          />
          <span className="hint">份（保底，绝不会清空整组）</span>
        </div>
      </label>

      <label className="check">
        <input
          type="checkbox"
          checked={config.neverDeleteProtected}
          onChange={(e) => onChange({ ...config, neverDeleteProtected: e.target.checked })}
        />
        <span>
          受保护文件永不标记删除
          <small>被素材库引用、硬链接同源、RAW/JPEG 配对成员</small>
        </span>
      </label>

      <label className="check">
        <input
          type="checkbox"
          checked={config.alsoMarkSimilar}
          onChange={(e) => onChange({ ...config, alsoMarkSimilar: e.target.checked })}
        />
        <span>
          相似组也自动标记
          <small>默认关闭：图片/音频相似组建议人工确认后再清理</small>
        </span>
      </label>
    </div>
  );
}
