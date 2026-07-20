import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import {
  loadLocalePreference,
  resolveLocale,
  saveLocalePreference,
  translate,
  type Locale,
  type LocalePreference,
} from './core';
import type { MessageKey } from './messages';

interface I18nValue {
  locale: Locale;
  preference: LocalePreference;
  setPreference: (value: LocalePreference) => void;
  t: (key: MessageKey, params?: Record<string, string | number>) => string;
}
const I18nContext = createContext<I18nValue | null>(null);

export function I18nProvider({ children }: { children: ReactNode }) {
  const [preference, setPreferenceState] = useState(() => loadLocalePreference(localStorage));
  const locale = resolveLocale(
    preference,
    navigator.languages.length ? navigator.languages : [navigator.language],
  );
  const value = useMemo<I18nValue>(
    () => ({
      locale,
      preference,
      setPreference(value) {
        setPreferenceState(value);
        saveLocalePreference(localStorage, value);
      },
      t: (key, params) => translate(locale, key, params),
    }),
    [locale, preference],
  );
  useEffect(() => {
    document.documentElement.lang = locale;
  }, [locale]);
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nValue {
  const value = useContext(I18nContext);
  if (!value) throw new Error('useI18n must be used inside I18nProvider');
  return value;
}
