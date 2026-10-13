import { useI18n, LOCALES, type Locale } from '../../i18n';
import { useTheme } from '../../theme';
import type { ThemeMode } from '../../theme';

const THEME_MODES: { value: ThemeMode; labelKey: 'appearance.themeLight' | 'appearance.themeDark' | 'appearance.themeSystem' }[] = [
  { value: 'light', labelKey: 'appearance.themeLight' },
  { value: 'dark', labelKey: 'appearance.themeDark' },
  { value: 'system', labelKey: 'appearance.themeSystem' },
];

export function AppearanceSettings() {
  const { t, locale, setLocale } = useI18n();
  const { mode, setMode } = useTheme();

  return (
    <div className="settings-form">
      <label className="field">
        <span>{t('appearance.language')}</span>
        <select value={locale} onChange={(e) => setLocale(e.target.value as Locale)}>
          {LOCALES.map((l) => (
            <option key={l.value} value={l.value}>
              {l.label}
            </option>
          ))}
        </select>
        <small className="hint">界面语言立即生效，无需重启。</small>
      </label>

      <div className="divider" />

      <label className="field">
        <span>{t('appearance.theme')}</span>
        <select value={mode} onChange={(e) => setMode(e.target.value as ThemeMode)}>
          {THEME_MODES.map((m) => (
            <option key={m.value} value={m.value}>
              {t(m.labelKey)}
            </option>
          ))}
        </select>
        <small className="hint">{t('appearance.themeHint')}</small>
      </label>
    </div>
  );
}
