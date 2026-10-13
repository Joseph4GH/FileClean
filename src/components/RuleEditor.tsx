import { useState } from 'react';

import { ruleLabel } from '../i18n/meta';
import { useI18n, type DictKey } from '../i18n';
import type { AutoMarkConfig, MarkRule, MarkRuleEntry } from '../types';
import { Icon } from './Icon';

const RULE_TYPES: MarkRule['type'][] = [
  'keepInPath',
  'preferExtension',
  'keepOldestCreated',
  'keepNewest',
  'keepOldest',
  'keepShortestPath',
  'keepLongestName',
  'keepShallowest',
  'keepLargest',
  'keepMostLinked',
];

export function RuleEditor({
  config,
  onChange,
}: {
  config: AutoMarkConfig;
  onChange: (next: AutoMarkConfig) => void;
}) {
  const { t } = useI18n();
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

  const ruleTypeKey = (type: MarkRule['type']) => `rule.type.${type}` as DictKey;

  return (
    <div>
      <div className="hint" style={{ marginBottom: 8 }}>
        {t('rule.orderHint')}
      </div>

      {config.rules.length === 0 && <div className="hint">{t('rule.empty')}</div>}

      {config.rules.map((entry, index) => (
        <div className="rule-item" key={`${entry.rule.type}-${index}`}>
          <input
            type="checkbox"
            checked={entry.enabled}
            onChange={(e) => update(index, { enabled: e.target.checked })}
            title={t('rule.toggleTitle')}
          />
          <span className="idx">{index + 1}</span>
          <span className="label" title={ruleLabel(entry.rule as never)}>
            {ruleLabel(entry.rule as never)}
          </span>
          <button className="btn ghost sm" onClick={() => move(index, -1)} title={t('rule.upTitle')}>
            <Icon name="up" size={13} />
          </button>
          <button className="btn ghost sm" onClick={() => move(index, 1)} title={t('rule.downTitle')}>
            <Icon name="down" size={13} />
          </button>
          <button className="btn ghost sm" onClick={() => remove(index)} title={t('common.delete')}>
            <Icon name="minus" size={13} />
          </button>
        </div>
      ))}

      {config.rules.map((entry, index) => {
        if (entry.rule.type === 'keepInPath') {
          return (
            <label className="field" key={`param-in-${index}`}>
              <span>{t('rule.inPathLabel', { n: index + 1 })}</span>
              <input
                type="text"
                value={entry.rule.pattern}
                onChange={(e) =>
                  update(index, { rule: { type: 'keepInPath', pattern: e.target.value } })
                }
                placeholder={t('rule.inPathPh')}
              />
            </label>
          );
        }
        if (entry.rule.type === 'preferExtension') {
          return (
            <label className="field" key={`param-ext-${index}`}>
              <span>{t('rule.extLabel', { n: index + 1 })}</span>
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
                placeholder={t('rule.extPh')}
              />
            </label>
          );
        }
        return null;
      })}

      {adding ? (
        <div className="card" style={{ marginTop: 8 }}>
          {RULE_TYPES.map((type) => (
            <div
              key={type}
              className="rule-item"
              style={{ cursor: 'pointer' }}
              onClick={() => add(type)}
            >
              <span className="label">
                {t(ruleTypeKey(type))}
                {t(`rule.type.${type}.desc` as DictKey) && (
                  <small className="hint" style={{ display: 'block' }}>
                    {t(`rule.type.${type}.desc` as DictKey)}
                  </small>
                )}
              </span>
              <Icon name="plus" size={13} />
            </div>
          ))}
          <button className="btn block sm" onClick={() => setAdding(false)}>
            {t('common.cancel')}
          </button>
        </div>
      ) : (
        <button className="btn block sm" onClick={() => setAdding(true)}>
          <Icon name="plus" size={13} /> {t('rule.add')}
        </button>
      )}

      <div className="divider" />

      <label className="field">
        <span>{t('rule.keepMin')}</span>
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
          <span className="hint">{t('rule.keepMinHint')}</span>
        </div>
      </label>

      <label className="check">
        <input
          type="checkbox"
          checked={config.neverDeleteProtected}
          onChange={(e) => onChange({ ...config, neverDeleteProtected: e.target.checked })}
        />
        <span>
          {t('rule.neverProtected')}
          <small>{t('rule.neverProtectedHint')}</small>
        </span>
      </label>

      <label className="check">
        <input
          type="checkbox"
          checked={config.alsoMarkSimilar}
          onChange={(e) => onChange({ ...config, alsoMarkSimilar: e.target.checked })}
        />
        <span>
          {t('rule.alsoSimilar')}
          <small>{t('rule.alsoSimilarHint')}</small>
        </span>
      </label>
    </div>
  );
}
