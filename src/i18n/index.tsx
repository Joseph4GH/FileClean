import { createContext, useContext, useEffect, useState, type ReactNode } from 'react';

import { zh, type DictKey } from './zh';
import { en } from './en';

export type { DictKey } from './zh';

export type Locale = 'zh' | 'en';
type Dict = Record<string, string>;

const DICTS: Record<Locale, Dict> = { zh, en };

/** 语言选项，供「外观」设置页渲染下拉。 */
export const LOCALES: { value: Locale; label: string }[] = [
  { value: 'zh', label: '中文' },
  { value: 'en', label: 'English' },
];

const STORAGE_KEY = 'fileclean.lang';

function initialLocale(): Locale {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    if (v === 'zh' || v === 'en') return v;
  } catch {
    /* localStorage 不可用时退回默认 */
  }
  return 'zh';
}

// 模块级活动语言。t() 直接读它，因此纯模块（format/meta）无需经过 React 即可翻译。
let activeLocale: Locale = initialLocale();
let activeDict: Dict = DICTS[activeLocale];

const listeners = new Set<(l: Locale) => void>();

export function getLocale(): Locale {
  return activeLocale;
}

/** 供纯模块（format / meta）在不经 React 时使用当前语言的字典。 */
export function getDict(): Dict {
  return activeDict;
}

export function setLocale(next: Locale): void {
  activeLocale = next;
  activeDict = DICTS[next];
  try {
    localStorage.setItem(STORAGE_KEY, next);
  } catch {
    /* 忽略持久化失败 */
  }
  listeners.forEach((fn) => fn(next));
}

/** 翻译。缺失 key 时回退为 key 本身，便于在界面上直接发现遗漏。 */
export function t(key: DictKey, vars?: Record<string, string | number>): string {
  const template = activeDict[key] ?? (key as string);
  if (!vars) return template;
  return template.replace(/\{(\w+)\}/g, (_m, k: string) =>
    k in vars ? String(vars[k]) : `{${k}}`,
  );
}

const LocaleContext = createContext<Locale>(activeLocale);

export function I18nProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(activeLocale);

  useEffect(() => {
    const fn = (l: Locale) => setLocaleState(l);
    listeners.add(fn);
    return () => {
      listeners.delete(fn);
    };
  }, []);

  return <LocaleContext.Provider value={locale}>{children}</LocaleContext.Provider>;
}

/** 在组件内取 t / 当前语言 / 切换函数。调用即订阅语言变化，语言切换时组件会重渲染。 */
export function useI18n() {
  const locale = useContext(LocaleContext);
  return { locale, setLocale, t };
}
