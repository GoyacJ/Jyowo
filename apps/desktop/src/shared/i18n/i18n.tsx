import i18next, { type i18n as I18n } from 'i18next'
import { type ReactNode, useEffect, useMemo } from 'react'
import { I18nextProvider, initReactI18next } from 'react-i18next'

import { useUiStore } from '@/shared/state/ui-store'

import { APP_LOCALES, type AppLocale, DEFAULT_APP_LOCALE, FALLBACK_APP_LOCALE } from './locales'
import { resources } from './resources'

export function createAppI18n(initialLocale: AppLocale = DEFAULT_APP_LOCALE): I18n {
  const instance = i18next.createInstance()

  void instance.use(initReactI18next).init({
    defaultNS: 'common',
    fallbackLng: FALLBACK_APP_LOCALE,
    initAsync: false,
    interpolation: {
      escapeValue: false,
    },
    lng: initialLocale,
    react: {
      useSuspense: false,
    },
    resources,
    supportedLngs: APP_LOCALES,
  })

  return instance
}

export const appI18n = createAppI18n()

export function AppI18nProvider({ children }: { children: ReactNode }) {
  const locale = useUiStore((state) => state.locale)
  const i18n = useMemo(() => appI18n, [])

  useEffect(() => {
    if (i18n.language === locale) {
      return
    }

    void i18n.changeLanguage(locale)
  }, [i18n, locale])

  return <I18nextProvider i18n={i18n}>{children}</I18nextProvider>
}
