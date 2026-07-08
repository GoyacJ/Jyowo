import { useNavigate, useRouterState } from '@tanstack/react-router'
import { Command as CommandIcon } from 'lucide-react'
import { type ReactNode, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ActivityRail } from '@/features/activity/ActivityRail'
import { OPEN_COMMAND_PALETTE_EVENT } from '@/features/workspace/CommandPalette'
import { SidebarNav } from '@/features/workspace/SidebarNav'
import { useUiStore } from '@/shared/state/ui-store'
import { Button } from '@/shared/ui/button'

export function AppShell({ children }: { children: ReactNode }) {
  const { t } = useTranslation(['shell', 'activity'])
  const navigate = useNavigate()
  const activeRunsByConversation = useUiStore((state) => state.activeRunsByConversation)
  const sidebarCollapsed = useUiStore((state) => state.sidebarCollapsed)
  const compactSidebar = useMediaQuery('(max-width: 720px)')
  const currentSearch = useRouterState({
    select: (state) => state.location.search,
  })
  const selectedConversationId =
    typeof currentSearch.conversationId === 'string' ? currentSearch.conversationId : undefined
  const selectedActiveRun = selectActiveRun(activeRunsByConversation, selectedConversationId)
  const sidebarWidth = sidebarCollapsed || compactSidebar ? '48px' : '300px'

  function openSettings() {
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
          gridTemplateColumns: `${sidebarWidth} minmax(0,1fr)`,
        }}
      >
        <SidebarNav compact={compactSidebar} />
        <div className="grid min-h-0 grid-rows-[52px_minmax(0,1fr)]">
          <header className="flex items-center justify-end gap-2 px-4">
            <Button
              aria-label={t('actions.openCommandPalette')}
              className="size-8"
              onClick={() => window.dispatchEvent(new Event(OPEN_COMMAND_PALETTE_EVENT))}
              size="icon"
              type="button"
              variant="outline"
            >
              <CommandIcon aria-hidden="true" className="size-4" />
            </Button>
          </header>
          <main className="min-h-0 min-w-0 overflow-hidden px-6 pb-6 xl:px-8">{children}</main>
        </div>
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
