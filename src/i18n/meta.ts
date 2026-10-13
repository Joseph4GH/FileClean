/**
 * 随语言变化的「元数据」字典：分组类型、决策、算法选项、规则标签。
 *
 * 原先这些结构直接写在 format.ts 里、文本硬编码。现在 label / hint 在调用
 * `t()` 时按当前语言取，tone（颜色档位）与 value（枚举）保持不变。
 * 这样组件渲染时会随语言切换自动拿到正确文案，且 tsc 强类型 key 能拦漏译。
 */

import type { DictKey } from './zh';
import { t } from './index';
import type { MatchKind, MarkDecision, MarkRule } from '../types';

const KIND_TONE: Record<MatchKind, 'accent' | 'warn' | 'ok' | ''> = {
  exact: 'accent',
  perceptual: 'warn',
  audio: 'warn',
  archiveInternal: '',
  rawJpegPair: 'ok',
};

export function kindMeta(kind: MatchKind): { label: string; tone: 'accent' | 'warn' | 'ok' | '' } {
  return { label: t(`kind.${kind}` as DictKey), tone: KIND_TONE[kind] };
}

const DECISION_TONE: Record<MarkDecision, 'ok' | 'danger' | ''> = {
  keep: 'ok',
  remove: 'danger',
  undecided: '',
};

export function decisionMeta(d: MarkDecision): { label: string; tone: 'ok' | 'danger' | '' } {
  return { label: t(`decision.${d}` as DictKey), tone: DECISION_TONE[d] };
}

export const ALGORITHM_VALUES = ['blake3', 'xxhash3', 'sha256', 'sha1', 'md5'] as const;
export type AlgoValue = (typeof ALGORITHM_VALUES)[number];

export interface AlgoOption {
  value: AlgoValue;
  label: string;
  hint: string;
}

export function algorithmOptions(): AlgoOption[] {
  return ALGORITHM_VALUES.map((v) => ({
    value: v,
    label: t(`algoOpt.${v}.label` as DictKey),
    hint: t(`algoOpt.${v}.hint` as DictKey),
  }));
}

export function ruleLabel(rule: { type: string; pattern?: string; extensions?: string[] }): string {
  switch (rule.type) {
    case 'keepNewest':
      return t('rule.type.keepNewest.label');
    case 'keepOldest':
      return t('rule.type.keepOldest.label');
    case 'keepOldestCreated':
      return t('rule.type.keepOldestCreated.label');
    case 'keepShortestPath':
      return t('rule.type.keepShortestPath.label');
    case 'keepLongestName':
      return t('rule.type.keepLongestName.label');
    case 'keepShallowest':
      return t('rule.type.keepShallowest.label');
    case 'keepLargest':
      return t('rule.type.keepLargest.label');
    case 'keepMostLinked':
      return t('rule.type.keepMostLinked.label');
    case 'keepInPath':
      return t('rule.type.keepInPath.full', { pattern: rule.pattern ?? '' });
    case 'preferExtension':
      return t('rule.type.preferExtension.full', {
        ext: (rule.extensions ?? []).join(' > ') || t('rule.type.preferExtension.label'),
      });
    default:
      return rule.type;
  }
}

export type { MarkRule };
