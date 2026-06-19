import { useRouterState } from '@tanstack/react-router'
import { MoreHorizontal, PanelRight, Share } from 'lucide-react'
import type { ReactNode } from 'react'
import { ActivityRail } from '@/features/activity/ActivityRail'
import { ReplayTimeline } from '@/features/activity/ReplayTimeline'
import { RunEventDetails } from '@/features/activity/RunEventDetails'
import { SupportBundleExport } from '@/features/activity/SupportBundleExport'
import { UsageSummary } from '@/features/activity/UsageSummary'
import { useActivity } from '@/features/activity/use-activity'
import { ContextPanel } from '@/features/context/ContextPanel'
import { useContextSnapshot } from '@/features/context/use-context-snapshot'
import { useConversation } from '@/features/conversation/use-conversation'
import { SidebarNav } from '@/features/workspace/SidebarNav'
import { useUiStore } from '@/shared/state/ui-store'
import { exportSupportBundle } from '@/shared/tauri/commands'
import { getCommandErrorMessage } from '@/shared/tauri/errors'
import { useCommandClient } from '@/shared/tauri/react'

export function AppShell({ children }: { children: ReactNode }) {
  const activityRailCollapsed = useUiStore((state) => state.activityRailCollapsed)
  const activityRailExpanded = useUiStore((state) => state.activityRailExpanded)
  const activeRunConversationId = useUiStore((state) => state.activeRunConversationId)
  const activeRunId = useUiStore((state) => state.activeRunId)
  const contextPanelCollapsed = useUiStore((state) => state.contextPanelCollapsed)
  const setContextPanelCollapsed = useUiStore((state) => state.setContextPanelCollapsed)
  const setActivityRailCollapsed = useUiStore((state) => state.setActivityRailCollapsed)
  const setActivityRailExpanded = useUiStore((state) => state.setActivityRailExpanded)
  const commandClient = useCommandClient()
  const selectedConversationIdFromSearch = useRouterState({
    select: (state) => selectedConversationIdFromSearchValue(state.location.search),
  })
  const conversation = useConversation({
    conversationId: selectedConversationIdFromSearch,
    includeDetail: false,
  })
  const selectedConversationId = conversation.selectedConversationId
  const activityRequest =
    activeRunId && selectedConversationId && activeRunConversationId === selectedConversationId
      ? { conversationId: selectedConversationId, runId: activeRunId }
      : selectedConversationId
        ? { conversationId: selectedConversationId }
        : {}
  const contextRequest = selectedConversationId ? { conversationId: selectedConversationId } : {}
  const activity = useActivity(activityRequest)
  const contextSnapshot = useContextSnapshot(contextRequest)
  const activityRailHeight = activityRailCollapsed
    ? '32px'
    : activityRailExpanded
      ? '360px'
      : '44px'
  const activityRail = (
    <ActivityRail
      collapsed={activityRailCollapsed}
      currentRun={activity.currentRun}
      errorMessage={activity.error ? getCommandErrorMessage(activity.error) : undefined}
      expanded={activityRailExpanded}
      items={activity.items}
      loading={activity.isLoading}
      onCollapse={() => {
        setActivityRailCollapsed(true)
        setActivityRailExpanded(false)
      }}
      onExpand={() => {
        setActivityRailCollapsed(false)
      }}
      onViewAll={() => {
        setActivityRailCollapsed(false)
        setActivityRailExpanded(true)
      }}
    />
  )

  return (
    <div
      className="grid h-screen min-h-0 min-w-0 overflow-hidden bg-background text-foreground"
      style={{ gridTemplateRows: `minmax(0, 1fr) ${activityRailHeight}` }}
    >
      <div
        className="grid min-h-0"
        style={{
          gridTemplateColumns: contextPanelCollapsed
            ? '268px minmax(0,1fr)'
            : '268px minmax(0,1fr) 320px',
        }}
      >
        <SidebarNav />
        <div className="grid min-h-0 grid-rows-[56px_minmax(0,1fr)]">
          <header className="flex items-center justify-end gap-2 px-6">
            <button
              aria-label="More actions"
              className="rounded-md p-2 text-muted-foreground hover:bg-muted hover:text-foreground"
              disabled
              type="button"
            >
              <MoreHorizontal className="size-4" />
            </button>
            <button
              className="flex items-center gap-2 rounded-md border border-border bg-surface px-3 py-1.5 text-sm hover:bg-muted"
              disabled
              type="button"
            >
              <Share className="size-4" />
              Share
            </button>
            <button
              aria-label={contextPanelCollapsed ? 'Show context panel' : 'Hide context panel'}
              aria-pressed={!contextPanelCollapsed}
              className="rounded-md border border-border bg-surface p-2 hover:bg-muted"
              onClick={() => setContextPanelCollapsed(!contextPanelCollapsed)}
              type="button"
            >
              <PanelRight className="size-4" />
            </button>
          </header>
          <main className="min-h-0 min-w-0 overflow-hidden px-8 pb-8 xl:px-16">{children}</main>
        </div>
        {contextPanelCollapsed ? null : (
          <ContextPanel
            context={contextSnapshot.context}
            errorMessage={
              contextSnapshot.error ? getCommandErrorMessage(contextSnapshot.error) : undefined
            }
            loading={contextSnapshot.isLoading}
            onClose={() => setContextPanelCollapsed(true)}
          />
        )}
      </div>
      {activityRailExpanded ? (
        <div className="grid min-h-0 grid-rows-[44px_minmax(0,1fr)] bg-background">
          {activityRail}
          <div className="grid min-h-0 grid-cols-[minmax(0,1fr)_minmax(320px,420px)] gap-6 overflow-auto border-border border-t px-6 py-4">
            {activity.activeDetails ? (
              <RunEventDetails
                event={activity.activeDetails}
                onApprovePermission={activity.approvePermission}
                onDenyPermission={activity.denyPermission}
                resolvingPermissionId={activity.resolvingPermissionId}
              />
            ) : (
              <section aria-label="Run event details" />
            )}
            <div className="space-y-6">
              <UsageSummary unavailable={!activity.usageSummary} usage={activity.usageSummary} />
              <ReplayTimeline events={activity.events} replayed />
              {activityRequest.conversationId ? (
                <SupportBundleExport
                  onExport={() => exportSupportBundle(activityRequest, commandClient)}
                />
              ) : null}
            </div>
          </div>
        </div>
      ) : (
        activityRail
      )}
    </div>
  )
}

function selectedConversationIdFromSearchValue(search: unknown) {
  if (typeof search !== 'object' || search === null || !('conversationId' in search)) {
    return undefined
  }

  const conversationId = search.conversationId
  return typeof conversationId === 'string' && conversationId.trim().length > 0
    ? conversationId
    : undefined
}
