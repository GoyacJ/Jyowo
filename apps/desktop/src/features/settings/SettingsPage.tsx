import { useNavigate, useRouterState } from '@tanstack/react-router'
import { Languages, Monitor, Moon, Sun } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { APP_LOCALES, type AppLocale } from '@/shared/i18n/locales'
import { cn } from '@/shared/lib/utils'
import { useUiStore } from '@/shared/state/ui-store'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/shared/ui/tabs'
import { AboutSettings } from './AboutSettings'
import { AutomationSettings } from './AutomationSettings'
import { ExecutionSettings } from './ExecutionSettings'
import { MCPManager } from './MCPManager'
import { ModelSettingsPage } from './models/ModelSettingsPage'
import { type PluginOpenRequest, PluginsManager } from './PluginsManager'
import { RuntimeExecutionStatusPanel } from './RuntimeExecutionStatusPanel'
import { BuiltinToolsList, SkillsManager } from './SkillSettings'

type SettingsTab =
  | 'general'
  | 'skills'
  | 'tools'
  | 'automations'
  | 'mcp'
  | 'plugins'
  | 'models'
  | 'about'

export function SettingsPage() {
  const { t } = useTranslation('settings')
  const navigate = useNavigate()
  const requestedTab = useRouterState({
    select: (state) => state.location.search.tab,
  })
  const [activeTab, setActiveTab] = useState<SettingsTab>(
    isSettingsTab(requestedTab) ? requestedTab : 'general',
  )
  const [openPluginRequest, setOpenPluginRequest] = useState<PluginOpenRequest | null>(null)

  useEffect(() => {
    if (isSettingsTab(requestedTab) && requestedTab !== activeTab) {
      setActiveTab(requestedTab)
    }
  }, [activeTab, requestedTab])

  function openPlugin(pluginId: string) {
    setOpenPluginRequest((current) => ({
      pluginId,
      requestId: (current?.requestId ?? 0) + 1,
    }))
    setActiveTab('plugins')
  }

  function selectTab(tab: SettingsTab) {
    setActiveTab(tab)
    void navigate({ search: { tab }, to: '/settings' })
  }

  return (
    <section aria-label={t('pageTitle')} className="h-full min-h-0 overflow-y-auto pr-1">
      <div className="mx-auto flex w-full max-w-6xl flex-col gap-5 pb-6">
        <Tabs
          className="min-h-0"
          onValueChange={(value) => {
            if (isSettingsTab(value)) {
              selectTab(value)
            }
          }}
          value={activeTab}
        >
          <TabsList aria-label={t('tabs.label')} className="flex h-auto w-fit flex-wrap">
            <TabsTrigger value="general">{t('tabs.general')}</TabsTrigger>
            <TabsTrigger value="skills">{t('tabs.skills')}</TabsTrigger>
            <TabsTrigger value="tools">{t('tabs.tools')}</TabsTrigger>
            <TabsTrigger value="automations">{t('tabs.automations')}</TabsTrigger>
            <TabsTrigger value="mcp">{t('tabs.mcp')}</TabsTrigger>
            <TabsTrigger value="plugins">{t('tabs.plugins')}</TabsTrigger>
            <TabsTrigger value="models">{t('tabs.models')}</TabsTrigger>
            <TabsTrigger value="about">{t('tabs.about')}</TabsTrigger>
          </TabsList>

          <TabsContent className="space-y-5 pt-3" value="general">
            <LanguageSettings />
            <ThemeSettings />
            <ExecutionSettings />
          </TabsContent>
          <TabsContent className="space-y-5 pt-3" value="skills">
            <SkillsManager onOpenPlugin={openPlugin} />
          </TabsContent>
          <TabsContent className="space-y-5 pt-3" value="tools">
            <RuntimeExecutionStatusPanel />
            <BuiltinToolsList />
          </TabsContent>
          <TabsContent className="space-y-5 pt-3" value="automations">
            <AutomationSettings />
          </TabsContent>
          <TabsContent className="space-y-5 pt-3" value="mcp">
            <MCPManager onOpenPlugin={openPlugin} />
          </TabsContent>
          <TabsContent className="space-y-5 pt-3" value="plugins">
            <PluginsManager openPluginRequest={openPluginRequest} />
          </TabsContent>
          <TabsContent className="space-y-5 pt-3" value="models">
            <ModelSettingsPage />
          </TabsContent>
          <TabsContent className="space-y-5 pt-3" value="about">
            <AboutSettings />
          </TabsContent>
        </Tabs>
      </div>
    </section>
  )
}

function isSettingsTab(value: unknown): value is SettingsTab {
  return (
    value === 'general' ||
    value === 'skills' ||
    value === 'tools' ||
    value === 'automations' ||
    value === 'mcp' ||
    value === 'plugins' ||
    value === 'models' ||
    value === 'about'
  )
}

const themeOptions = [
  { value: 'light', icon: Sun },
  { value: 'dark', icon: Moon },
  { value: 'system', icon: Monitor },
] as const

function ThemeSettings() {
  const { t } = useTranslation('settings')
  const theme = useUiStore((state) => state.theme)
  const setTheme = useUiStore((state) => state.setTheme)

  return (
    <section className="space-y-5 rounded-md border border-border bg-surface p-5">
      <div className="flex items-start gap-3">
        <div className="rounded-md border border-border bg-background p-2 text-muted-foreground">
          <Sun className="size-4" />
        </div>
        <div>
          <h2 className="font-semibold text-base">{t('theme.title')}</h2>
          <p className="mt-1 text-muted-foreground text-sm">{t('theme.description')}</p>
        </div>
      </div>

      <fieldset className="flex w-fit flex-wrap gap-2">
        <legend className="sr-only">{t('theme.label')}</legend>
        {themeOptions.map(({ icon: Icon, value }) => (
          <button
            aria-pressed={theme === value}
            className={cn(
              'inline-flex h-9 items-center gap-2 rounded-sm border border-border px-3 font-medium text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
              theme === value
                ? 'bg-primary text-primary-foreground'
                : 'bg-background text-muted-foreground hover:bg-muted hover:text-foreground',
            )}
            key={value}
            onClick={() => setTheme(value)}
            type="button"
          >
            <Icon aria-hidden="true" className="size-4" data-icon />
            {t(`theme.options.${value}`)}
          </button>
        ))}
      </fieldset>
    </section>
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
