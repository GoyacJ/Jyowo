import { useNavigate, useRouterState } from '@tanstack/react-router'
import { type ReactNode, useEffect, useState } from 'react'
import { useStore } from 'zustand'
import { ActivityRail } from '@/features/activity/ActivityRail'
import { deriveLiveTaskSnapshot } from '@/features/tasks/task-live-projection'
import { taskStoreFor } from '@/features/tasks/use-task'
import { SidebarDivider } from '@/features/workspace/SidebarDivider'
import { SidebarNav } from '@/features/workspace/SidebarNav'
import { COLLAPSED_SIDEBAR_WIDTH } from '@/shared/state/sidebar-layout'
import { useUiStore } from '@/shared/state/ui-store'

export function AppShell({ children }: { children: ReactNode }) {
  const navigate = useNavigate()
  const activeRunsByConversation = useUiStore((state) => state.activeRunsByConversation)
  const sidebarCollapsed = useUiStore((state) => state.sidebarCollapsed)
  const sidebarWidth = useUiStore((state) => state.sidebarWidth)
  const setSidebarCollapsed = useUiStore((state) => state.setSidebarCollapsed)
  const setSidebarWidth = useUiStore((state) => state.setSidebarWidth)
  const compactSidebar = useMediaQuery('(max-width: 720px)')
  const currentSearch = useRouterState({
    select: (state) => state.location.search,
  })
  const pathname = useRouterState({
    select: (state) => state.location.pathname,
  })
  const selectedTaskId = typeof currentSearch.taskId === 'string' ? currentSearch.taskId : undefined
  const selectedActiveRun = selectOnlyActiveRun(activeRunsByConversation)
  const renderedSidebarWidth =
    sidebarCollapsed || compactSidebar ? COLLAPSED_SIDEBAR_WIDTH : sidebarWidth

  function openSettings() {
    void navigate({ to: '/settings' })
  }

  return (
    <div
      className="relative grid h-screen min-h-0 min-w-0 overflow-hidden bg-background text-foreground"
      style={{ gridTemplateRows: 'minmax(0, 1fr) 32px' }}
    >
      <div
        className="relative grid min-h-0"
        style={{
          gridTemplateColumns: `${renderedSidebarWidth}px minmax(0,1fr)`,
        }}
      >
        <SidebarNav compact={compactSidebar} />
        <SidebarDivider
          collapsed={sidebarCollapsed}
          compact={compactSidebar}
          onCollapsedChange={setSidebarCollapsed}
          onWidthChange={setSidebarWidth}
          width={sidebarWidth}
        />
        <div className="grid min-h-0 grid-rows-[52px_minmax(0,1fr)]">
          <header className="flex min-w-0 items-center justify-between gap-3 px-6 xl:px-8">
            {pathname === '/' && selectedTaskId ? (
              <SelectedTaskTitle taskId={selectedTaskId} />
            ) : (
              <span aria-hidden="true" />
            )}
          </header>
          <main className="min-h-0 min-w-0 overflow-hidden px-6 pb-6 xl:px-8">
            <div key={pathname} className="h-full min-h-0 min-w-0 animate-page-enter">
              {children}
            </div>
          </main>
        </div>
      </div>
      {pathname === '/' && selectedTaskId ? (
        <SelectedTaskActivityRail onOpenSettings={openSettings} taskId={selectedTaskId} />
      ) : (
        <ActivityRail activeRunId={selectedActiveRun?.runId} onOpenSettings={openSettings} />
      )}
    </div>
  )
}

function SelectedTaskTitle({ taskId }: { taskId: string }) {
  const store = taskStoreFor(taskId)
  const snapshot = useStore(store, (state) => state.snapshot)
  const events = useStore(store, (state) => state.events)
  const title = snapshot ? deriveLiveTaskSnapshot(snapshot, events).projection.title : null

  return title ? (
    <h1 className="min-w-0 flex-1 truncate font-semibold text-sm tracking-[-0.01em]">{title}</h1>
  ) : (
    <span aria-hidden="true" className="min-w-0 flex-1" />
  )
}

function SelectedTaskActivityRail({
  onOpenSettings,
  taskId,
}: {
  onOpenSettings: () => void
  taskId: string
}) {
  const activeRunId = useStore(
    taskStoreFor(taskId),
    (state) => state.snapshot?.projection.currentRun?.segmentId,
  )

  return <ActivityRail activeRunId={activeRunId} onOpenSettings={onOpenSettings} />
}

function selectOnlyActiveRun(activeRunsByConversation: Record<string, string>) {
  const activeRuns = Object.entries(activeRunsByConversation)

  if (activeRuns.length !== 1) {
    return undefined
  }

  const [conversationId, runId] = activeRuns[0] ?? []

  if (!conversationId || !runId) {
    return undefined
  }

  return { conversationId, runId }
}

function useMediaQuery(query: string) {
  const [matches, setMatches] = useState(() => window.matchMedia?.(query).matches ?? false)

  useEffect(() => {
    const mediaQuery = window.matchMedia?.(query)

    if (!mediaQuery) {
      return
    }

    const updateMatches = () => setMatches(mediaQuery.matches)

    updateMatches()
    mediaQuery.addEventListener('change', updateMatches)

    return () => mediaQuery.removeEventListener('change', updateMatches)
  }, [query])

  return matches
}
