import { useNavigate, useRouterState } from '@tanstack/react-router'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { SkillCatalogManager } from '@/features/skills/catalog/SkillCatalogManager'
import { SkillConfigPanel } from '@/features/skills/config/SkillConfigPanel'
import { InstalledSkillsManager } from '@/features/skills/installed/InstalledSkillsManager'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/shared/ui/tabs'

import { MCPManager } from './MCPManager'
import { type PluginOpenRequest, PluginsManager } from './PluginsManager'
import { RuntimeToolsSettings } from './RuntimeToolsSettings'

type SkillSettingsTab = 'skills' | 'tools' | 'mcp' | 'plugins'

export function SkillSettingsPage() {
  const { t } = useTranslation('skills')
  const navigate = useNavigate()
  const requestedTab = useRouterState({ select: (state) => state.location.search.tab })
  const [activeTab, setActiveTab] = useState<SkillSettingsTab>(
    isSkillSettingsTab(requestedTab) ? requestedTab : 'skills',
  )
  const [openPluginRequest, setOpenPluginRequest] = useState<PluginOpenRequest | null>(null)

  useEffect(() => {
    if (isSkillSettingsTab(requestedTab) && requestedTab !== activeTab) {
      setActiveTab(requestedTab)
    }
  }, [activeTab, requestedTab])

  function selectTab(tab: SkillSettingsTab) {
    setActiveTab(tab)
    void navigate({ search: { tab }, to: '/skills' })
  }

  function openPlugin(pluginId: string) {
    setOpenPluginRequest((current) => ({
      pluginId,
      requestId: (current?.requestId ?? 0) + 1,
    }))
    selectTab('plugins')
  }

  return (
    <section aria-label={t('pageTitle')} className="h-full min-h-0 overflow-y-auto pr-1">
      <div className="mx-auto flex w-full max-w-5xl flex-col gap-3 pb-6">
        <Tabs
          className="min-h-0"
          onValueChange={(value) => {
            if (isSkillSettingsTab(value)) {
              selectTab(value)
            }
          }}
          value={activeTab}
        >
          <TabsList aria-label={t('tabs.label')}>
            <TabsTrigger value="skills">{t('tabs.skills')}</TabsTrigger>
            <TabsTrigger value="tools">{t('tabs.tools')}</TabsTrigger>
            <TabsTrigger value="mcp">{t('tabs.mcp')}</TabsTrigger>
            <TabsTrigger value="plugins">{t('tabs.plugins')}</TabsTrigger>
          </TabsList>
          <TabsContent className="space-y-5 pt-3" value="skills">
            <SkillsManager onOpenPlugin={openPlugin} />
          </TabsContent>
          <TabsContent className="space-y-5 pt-3" value="tools">
            <RuntimeToolsList />
          </TabsContent>
          <TabsContent className="space-y-5 pt-3" value="mcp">
            <MCPManager onOpenPlugin={openPlugin} />
          </TabsContent>
          <TabsContent className="space-y-5 pt-3" value="plugins">
            <PluginsManager openPluginRequest={openPluginRequest} />
          </TabsContent>
        </Tabs>
      </div>
    </section>
  )
}

function isSkillSettingsTab(value: unknown): value is SkillSettingsTab {
  return value === 'skills' || value === 'tools' || value === 'mcp' || value === 'plugins'
}

export function SkillsManager({
  onOpenPlugin,
}: {
  onOpenPlugin?: (pluginId: string) => void
} = {}) {
  const { t } = useTranslation('skills')

  return (
    <Tabs className="min-h-0" defaultValue="installed">
      <TabsList aria-label={t('managerTabs.label')}>
        <TabsTrigger value="installed">{t('managerTabs.installed')}</TabsTrigger>
        <TabsTrigger value="catalog">{t('managerTabs.catalog')}</TabsTrigger>
      </TabsList>
      <TabsContent className="space-y-5 pt-3" value="installed">
        <InstalledSkillsManager
          onOpenPlugin={onOpenPlugin}
          renderConfig={(skillId) => <SkillConfigPanel skillId={skillId} />}
        />
      </TabsContent>
      <TabsContent className="space-y-5 pt-3" value="catalog">
        <SkillCatalogManager />
      </TabsContent>
    </Tabs>
  )
}

export function RuntimeToolsList() {
  return <RuntimeToolsSettings />
}
