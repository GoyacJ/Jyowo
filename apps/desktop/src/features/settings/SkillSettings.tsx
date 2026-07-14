import { useQuery } from '@tanstack/react-query'
import { useNavigate, useRouterState } from '@tanstack/react-router'
import { Search, Wrench } from 'lucide-react'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { SkillCatalogManager } from '@/features/skills/catalog/SkillCatalogManager'
import { SkillConfigPanel } from '@/features/skills/config/SkillConfigPanel'
import { InstalledSkillsManager } from '@/features/skills/installed/InstalledSkillsManager'
import { listRuntimeTools, type RuntimeToolSummary } from '@/shared/tauri/commands'
import { getCommandErrorMessage } from '@/shared/tauri/errors'
import { useCommandClient } from '@/shared/tauri/react'
import { Badge } from '@/shared/ui/badge'
import { Input } from '@/shared/ui/input'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/shared/ui/tabs'

import { MCPManager } from './MCPManager'
import { type PluginOpenRequest, PluginsManager } from './PluginsManager'

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
  const { t } = useTranslation('skills')
  const commandClient = useCommandClient()
  const [query, setQuery] = useState('')
  const toolsQuery = useQuery({
    queryKey: ['runtime-tools'],
    queryFn: () => listRuntimeTools(commandClient),
  })
  const tools = toolsQuery.data?.tools ?? []
  const normalizedQuery = query.trim().toLowerCase()
  const groupLabelForTool = useCallback(
    (tool: RuntimeToolSummary) =>
      t(`tools.groups.${tool.group}`, { defaultValue: tool.groupLabel }),
    [t],
  )
  const filteredTools = useMemo(() => {
    if (!normalizedQuery) {
      return tools
    }
    return tools.filter((tool) =>
      [
        tool.name,
        tool.displayName,
        tool.description,
        tool.group,
        groupLabelForTool(tool),
        tool.originKind,
        tool.originId ?? '',
        tool.executionChannel,
      ]
        .join(' ')
        .toLowerCase()
        .includes(normalizedQuery),
    )
  }, [groupLabelForTool, normalizedQuery, tools])

  return (
    <section className="rounded-md border border-border bg-surface">
      <div className="flex items-start justify-between gap-4 border-border border-b p-5">
        <div className="flex items-start gap-3">
          <div className="rounded-md border border-border bg-background p-2 text-muted-foreground">
            <Wrench className="size-4" />
          </div>
          <div>
            <h2 className="font-semibold text-base">{t('tools.title')}</h2>
            <p className="mt-1 text-muted-foreground text-sm">{t('tools.description')}</p>
          </div>
        </div>
        <Badge className="mt-0.5" variant="secondary">
          {t('tools.count', { count: tools.length })}
        </Badge>
      </div>

      <div className="border-border border-b p-4">
        <div className="relative">
          <Search className="-translate-y-1/2 pointer-events-none absolute top-1/2 left-3 size-4 text-muted-foreground" />
          <Input
            aria-label={t('tools.searchLabel')}
            className="pl-9"
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t('tools.searchPlaceholder')}
            value={query}
          />
        </div>
      </div>

      {toolsQuery.isLoading ? (
        <p className="p-5 text-muted-foreground text-sm">{t('tools.loading')}</p>
      ) : toolsQuery.isError ? (
        <p className="p-5 text-destructive text-sm">{getCommandErrorMessage(toolsQuery.error)}</p>
      ) : filteredTools.length === 0 ? (
        <p className="p-5 text-muted-foreground text-sm">{t('tools.empty')}</p>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full min-w-[960px] border-collapse text-left text-sm">
            <thead className="bg-background text-muted-foreground">
              <tr className="border-border border-b">
                <th className="px-5 py-3 font-medium">{t('tools.columns.tool')}</th>
                <th className="px-5 py-3 font-medium">{t('tools.columns.group')}</th>
                <th className="px-5 py-3 font-medium">{t('tools.columns.origin')}</th>
                <th className="px-5 py-3 font-medium">{t('tools.columns.access')}</th>
                <th className="px-5 py-3 font-medium">{t('tools.columns.execution')}</th>
                <th className="px-5 py-3 font-medium">{t('tools.columns.description')}</th>
              </tr>
            </thead>
            <tbody>
              {filteredTools.map((tool) => (
                <tr className="border-border border-b last:border-b-0" key={tool.name}>
                  <td className="px-5 py-3 align-top">
                    <div className="font-medium text-foreground">{tool.displayName}</div>
                    {tool.name !== tool.displayName ? (
                      <div className="mt-0.5 font-mono text-muted-foreground text-xs">
                        {tool.name}
                      </div>
                    ) : null}
                  </td>
                  <td className="px-5 py-3 align-top text-muted-foreground">
                    {groupLabelForTool(tool)}
                  </td>
                  <td className="px-5 py-3 align-top">
                    <div className="text-muted-foreground">
                      {t(`tools.origin.${tool.originKind}`)}
                    </div>
                    {tool.originId ? (
                      <div className="mt-0.5 max-w-40 truncate font-mono text-muted-foreground text-xs">
                        {tool.originId}
                      </div>
                    ) : null}
                  </td>
                  <td className="px-5 py-3 align-top">
                    <Badge variant={accessBadgeVariant(tool.access)}>
                      {t(`tools.access.${tool.access}`)}
                    </Badge>
                  </td>
                  <td className="px-5 py-3 align-top text-muted-foreground">
                    {t(`tools.execution.${tool.executionChannel}`)}
                  </td>
                  <td className="max-w-md px-5 py-3 align-top text-muted-foreground">
                    {tool.description}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  )
}

function accessBadgeVariant(access: RuntimeToolSummary['access']) {
  if (access === 'destructive') return 'destructive'
  if (access === 'readOnly') return 'secondary'
  return 'outline'
}
