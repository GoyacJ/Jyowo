import { createLazyFileRoute, useNavigate, useRouterState } from '@tanstack/react-router'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { MemoryBrowser } from '@/features/memory/MemoryBrowser'
import { MemoryInbox } from '@/features/memory/MemoryInbox'
import { MemoryRecallTracePanel } from '@/features/memory/MemoryRecallTracePanel'
import { MemorySettings } from '@/features/memory/MemorySettings'
import { useActiveProjectPath } from '@/features/workspace/use-active-project-path'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/shared/ui/tabs'

type MemoryTab = 'items' | 'inbox' | 'traces' | 'settings'

function MemoryPage() {
  const { t } = useTranslation('memory')
  const navigate = useNavigate()
  const requestedTab = useRouterState({
    select: (state) => state.location.search.tab,
  })
  const requestedWorkspaceRoot = useRouterState({
    select: (state) => state.location.search.workspaceRoot,
  })
  const activeProjectPathQuery = useActiveProjectPath({ enabled: !requestedWorkspaceRoot })
  const workspaceRoot = requestedWorkspaceRoot ?? activeProjectPathQuery.data ?? undefined
  const [tab, setTab] = useState<MemoryTab>(isMemoryTab(requestedTab) ? requestedTab : 'items')

  const tabs: { id: MemoryTab; label: string }[] = [
    { id: 'items', label: t('items') },
    { id: 'inbox', label: t('inbox') },
    { id: 'traces', label: t('recallTraces') },
    { id: 'settings', label: t('settings') },
  ]

  useEffect(() => {
    if (isMemoryTab(requestedTab) && requestedTab !== tab) {
      setTab(requestedTab)
    }
  }, [requestedTab, tab])

  function selectTab(nextTab: MemoryTab) {
    setTab(nextTab)
    void navigate({ search: { tab: nextTab, workspaceRoot }, to: '/memory' })
  }

  if (!requestedWorkspaceRoot && activeProjectPathQuery.isLoading) {
    return <div className="text-muted-foreground text-sm">{t('loading')}</div>
  }

  if (!requestedWorkspaceRoot && activeProjectPathQuery.isError) {
    return <div className="text-destructive text-sm">{t('errorLoading')}</div>
  }

  return (
    <section aria-label={t('title')} className="h-full min-h-0 overflow-y-auto pr-1">
      <div className="mx-auto flex w-full max-w-5xl flex-col gap-3 pb-6">
        <Tabs
          className="min-h-0"
          onValueChange={(value) => {
            if (isMemoryTab(value)) {
              selectTab(value)
            }
          }}
          value={tab}
        >
          <TabsList aria-label={t('tabsLabel')} className="flex h-auto w-fit flex-wrap">
            {tabs.map((item) => (
              <TabsTrigger key={item.id} value={item.id}>
                {item.label}
              </TabsTrigger>
            ))}
          </TabsList>
          <TabsContent className="space-y-5 pt-3" value="items">
            <MemoryBrowser workspaceRoot={workspaceRoot} />
          </TabsContent>
          <TabsContent className="space-y-5 pt-3" value="inbox">
            <MemoryInbox workspaceRoot={workspaceRoot} />
          </TabsContent>
          <TabsContent className="space-y-5 pt-3" value="traces">
            <MemoryRecallTracePanel workspaceRoot={workspaceRoot} />
          </TabsContent>
          <TabsContent className="space-y-5 pt-3" value="settings">
            <MemorySettings workspaceRoot={workspaceRoot} />
          </TabsContent>
        </Tabs>
      </div>
    </section>
  )
}

function isMemoryTab(value: unknown): value is MemoryTab {
  return value === 'items' || value === 'inbox' || value === 'traces' || value === 'settings'
}

export const Route = createLazyFileRoute('/memory')({
  component: MemoryPage,
})
