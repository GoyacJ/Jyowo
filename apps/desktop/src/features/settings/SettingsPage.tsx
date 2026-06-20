import { Languages } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { APP_LOCALES, type AppLocale } from '@/shared/i18n/locales'
import { useUiStore } from '@/shared/state/ui-store'

import { MCPManager } from './MCPManager'
import { ProviderSettingsForm } from './ProviderSettingsForm'

export function SettingsPage() {
  const { t } = useTranslation('settings')

  return (
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-5">
      <header>
        <h1 className="font-semibold text-2xl">{t('pageTitle')}</h1>
        <p className="mt-1 text-muted-foreground text-sm">{t('pageDescription')}</p>
      </header>

      <LanguageSettings />
      <ProviderSettingsForm />
      <MCPManager />
    </div>
  )
}

function LanguageSettings() {
  const { t } = useTranslation('settings')
  const locale = useUiStore((state) => state.locale)
  const setLocale = useUiStore((state) => state.setLocale)

  return (
    <section className="space-y-5 rounded-md border border-border bg-surface p-5">
      <div className="flex items-start gap-3">
        <div className="rounded-md border border-border bg-background p-2 text-muted-foreground">
          <Languages className="size-4" />
        </div>
        <div>
          <h2 className="font-semibold text-base">{t('language.title')}</h2>
          <p className="mt-1 text-muted-foreground text-sm">{t('language.description')}</p>
        </div>
      </div>

      <label className="block max-w-sm space-y-2 text-sm">
        <span className="font-medium">{t('language.label')}</span>
        <select
          className="h-10 w-full rounded-md border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
          onChange={(event) => setLocale(event.target.value as AppLocale)}
          value={locale}
        >
          {APP_LOCALES.map((appLocale) => (
            <option key={appLocale} value={appLocale}>
              {appLocale === 'zh-CN' ? t('language.zhCN') : t('language.enUS')}
            </option>
          ))}
        </select>
      </label>
    </section>
  )
}
