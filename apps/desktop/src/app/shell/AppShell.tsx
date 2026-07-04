import { useNavigate, useRouterState } from '@tanstack/react-router'
import { Command as CommandIcon, PanelRightOpen } from 'lucide-react'
import { type ReactNode, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ActivityRail } from '@/features/activity/ActivityRail'
import { ContextPanel } from '@/features/context/ContextPanel'
import { useContextSnapshot } from '@/features/context/use-context-snapshot'
import { WorkbenchInspector } from '@/features/workbench/WorkbenchInspector'
import { OPEN_COMMAND_PALETTE_EVENT } from '@/features/workspace/CommandPalette'
import { SidebarNav } from '@/features/workspace/SidebarNav'
import { useUiStore } from '@/shared/state/ui-store'
import { getCommandErrorMessage } from '@/shared/tauri/errors'
import { Button } from '@/shared/ui/button'

export function AppShell({ children }: { children: ReactNode }) {
  const { t } = useTranslation(['shell', 'activity'])
  const navigate = useNavigate()
  const activeRunsByConversation = useUiStore((state) => state.activeRunsByConversation)
  const sidebarCollapsed = useUiStore((state) => state.sidebarCollapsed)
  const contextPanelCollapsed = useUiStore((state) => state.contextPanelCollapsed)
  const compactSidebar = useMediaQuery('(max-width: 720px)')
  const inspectorOpen = useUiStore((state) => state.inspectorOpen)
  const setInspectorOpen = useUiStore((state) => state.setInspectorOpen)
  const setContextPanelCollapsed = useUiStore((state) => state.setContextPanelCollapsed)
  const requestTimelineScroll = useUiStore((state) => state.requestTimelineScroll)
  const setWorkbenchSelection = useUiStore((state) => state.setWorkbenchSelection)
  const currentPath = useRouterState({
    select: (state) => state.location.pathname,
  })
  const currentSearch = useRouterState({
    select: (state) => state.location.search,
  })
  const selectedConversationId =
    typeof currentSearch.conversationId === 'string' ? currentSearch.conversationId : undefined
  const selectedActiveRun = selectActiveRun(activeRunsByConversation, selectedConversationId)
  const contextAvailable = currentPath === '/'
  const contextVisible = contextAvailable && !contextPanelCollapsed
  const contextRequest =
    contextVisible && selectedActiveRun
      ? { conversationId: selectedActiveRun.conversationId, runId: selectedActiveRun.runId }
      : contextVisible && selectedConversationId
        ? { conversationId: selectedConversationId }
        : {}
  const contextSnapshot = useContextSnapshot(contextRequest, { enabled: contextVisible })
  const workbenchSelection = useUiStore((state) => state.workbenchSelection)
  const sidebarWidth = sidebarCollapsed || compactSidebar ? '48px' : '248px'
  const showInspector = inspectorOpen
  const showContext = contextVisible

  function openSettings() {
    setInspectorOpen(true)
    void navigate({ to: '/settings' })
  }

  return (
    <div
      className="relative grid h-screen min-h-0 min-w-0 overflow-hidden bg-background text-foreground"
      style={{ gridTemplateRows: 'minmax(0, 1fr) 32px' }}
    >
      <div
        className="grid min-h-0"
        style={{
          gridTemplateColumns: showInspector
            ? `${sidebarWidth} minmax(0,1fr) 360px`
            : showContext
              ? `${sidebarWidth} minmax(0,1fr) 320px`
              : `${sidebarWidth} minmax(0,1fr)`,
        }}
      >
        <SidebarNav compact={compactSidebar} />
        <div className="grid min-h-0 grid-rows-[52px_minmax(0,1fr)]">
          <header className="flex items-center justify-end gap-2 px-4">
            {contextAvailable && contextPanelCollapsed ? (
              <Button
                aria-label={t('actions.showContextPanel')}
                className="size-8"
                onClick={() => setContextPanelCollapsed(false)}
                size="icon"
                title={t('actions.showContextPanel')}
                type="button"
                variant="outline"
              >
                <PanelRightOpen className="size-4" />
              </Button>
            ) : null}
            <Button
              aria-label={t('actions.openCommandPalette')}
              className="size-8"
              onClick={() => window.dispatchEvent(new Event(OPEN_COMMAND_PALETTE_EVENT))}
              size="icon"
              type="button"
              variant="outline"
            >
              <CommandIcon className="size-4" />
            </Button>
            <Button
              aria-label={
                inspectorOpen
                  ? t('actions.closeInspector')
                  : t('actions.openInspector')
              }
              className="size-8"
              onClick={() => {
                if (inspectorOpen) {
                  setWorkbenchSelection(null)
                }
                setInspectorOpen(!inspectorOpen)
              }}
              size="icon"
              type="button"
              variant="outline"
            >
              <PanelRightOpen
                className={`size-4 ${inspectorOpen ? 'rotate-180' : ''}`}
              />
            </Button>
          </header>
          <main className="min-h-0 min-w-0 overflow-hidden px-6 pb-6 xl:px-8">{children}</main>
        </div>
        {showInspector ? (
          <WorkbenchInspector />
        ) : showContext ? (
          <ContextPanel
            context={contextSnapshot.context}
            errorMessage={
              contextSnapshot.error ? getCommandErrorMessage(contextSnapshot.error) : undefined
            }
            loading={contextSnapshot.isLoading}
            onClose={() => setContextPanelCollapsed(true)}
            onDecisionSelect={(decision) => {
              if (decision.requestId) {
                requestTimelineScroll(`permission:${decision.requestId}`)
              }
            }}
          />
        ) : null}
      </div>
      <ActivityRail activeRunId={selectedActiveRun?.runId} onOpenSettings={openSettings} />
    </div>
  )
}

function selectActiveRun(
  activeRunsByConversation: Record<string, string>,
  selectedConversationId: string | undefined,
) {
  if (selectedConversationId) {
    const runId = activeRunsByConversation[selectedConversationId]

    if (runId) {
      return {
        conversationId: selectedConversationId,
        runId,
      }
    }

    return undefined
  }

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
