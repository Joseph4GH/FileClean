import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from 'react';

export type ThemeMode = 'light' | 'dark' | 'system';
type Resolved = 'light' | 'dark';

const STORAGE_KEY = 'fileclean.theme';

function initialMode(): ThemeMode {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    if (v === 'light' || v === 'dark' || v === 'system') return v;
  } catch {
    /* 忽略持久化读取失败 */
  }
  return 'system';
}

function systemPrefersDark(): boolean {
  return typeof window !== 'undefined' && typeof window.matchMedia === 'function'
    ? window.matchMedia('(prefers-color-scheme: dark)').matches
    : false;
}

function resolve(mode: ThemeMode, systemDark: boolean): Resolved {
  if (mode === 'system') return systemDark ? 'dark' : 'light';
  return mode;
}

function applyTheme(resolved: Resolved): void {
  document.documentElement.setAttribute('data-theme', resolved);
}

// 模块级即时应用：首帧渲染前就把主题落到 <html>，避免浅色闪一下。
const initMode = initialMode();
let currentSystemDark = systemPrefersDark();
applyTheme(resolve(initMode, currentSystemDark));

interface ThemeCtx {
  mode: ThemeMode;
  setMode: (m: ThemeMode) => void;
}

const ThemeContext = createContext<ThemeCtx>({ mode: initMode, setMode: () => {} });

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [mode, setModeState] = useState<ThemeMode>(initMode);
  const modeRef = useRef<ThemeMode>(mode);
  modeRef.current = mode;

  useEffect(() => {
    if (typeof window.matchMedia !== 'function') return;
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    const onChange = (e: MediaQueryListEvent) => {
      currentSystemDark = e.matches;
      applyTheme(resolve(modeRef.current, currentSystemDark));
    };
    mq.addEventListener('change', onChange);
    return () => mq.removeEventListener('change', onChange);
  }, []);

  const setMode = (next: ThemeMode) => {
    setModeState(next);
    modeRef.current = next;
    try {
      localStorage.setItem(STORAGE_KEY, next);
    } catch {
      /* 忽略持久化失败 */
    }
    applyTheme(resolve(next, currentSystemDark));
  };

  return <ThemeContext.Provider value={{ mode, setMode }}>{children}</ThemeContext.Provider>;
}

export function useTheme(): ThemeCtx {
  return useContext(ThemeContext);
}
