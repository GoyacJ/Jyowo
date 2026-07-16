import {
  ArrowLeft,
  Bot,
  FileDiff,
  FileText,
  FolderGit2,
  Globe2,
  GripVertical,
  ImageIcon,
  ListTree,
  MessageSquareReply,
  Pin,
  PinOff,
  SquareTerminal,
  X,
} from 'lucide-react'
import { type CSSProperties, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { ArtifactRenderer } from '@/features/artifacts/ArtifactRenderer'
import type { ArtifactDescriptor } from '@/features/artifacts/model'
import { useArtifactResource } from '@/features/artifacts/resource'
import type {
  TaskEventEnvelope,
  TaskProjection,
  TimelineItemProjection,
} from '@/generated/daemon-protocol'
import type { DaemonClient } from '@/shared/daemon/client'
import { useUiStore } from '@/shared/state/ui-store'
import type {
  TaskWorkbenchTab,
  TaskWorkbenchTarget,
  TaskWorkbenchTargetKind,
} from '@/shared/state/workbench-selection'
import { Button } from '@/shared/ui/button'

import { AuditPanel } from './AuditPanel'
import { BrowserPanel } from './BrowserPanel'
import { CommandPanel } from './CommandPanel'
import { ArtifactText, DiffPanel } from './DiffPanel'
import { EnvironmentPanel } from './EnvironmentPanel'
import { SourcesPanel } from './SourcesPanel'
import { SubagentsPanel } from './SubagentsPanel'
import { isTaskWorkbenchSidebarTarget } from './task-workbench-target'

export function TaskWorkbench({
  client,
  events,
  onClosed,
  onLocateInTimeline,
  projection,
  snapshotOffset = projection.lastGlobalOffset,
  timeline = [],
}: {
  client: Pick<DaemonClient, 'loadTaskEvents' | 'readBlob' | 'request'>
  events: TaskEventEnvelope[]
  onClosed?: () => void
  onLocateInTimeline?: (eventId: string) => void
  projection: TaskProjection
  snapshotOffset?: number
  timeline?: TimelineItemProjection[]
}) {
  const { t } = useTranslation('tasks')
  const session = useUiStore((state) => state.taskWorkbenchByTaskId[projection.taskId])
  const width = useUiStore((state) => state.taskWorkbenchWidth)
  const activateTab = useUiStore((state) => state.activateTaskWorkbenchTab)
  const closeTab = useUiStore((state) => state.closeTaskWorkbenchTab)
  const closeWorkbench = useUiStore((state) => state.closeTaskWorkbench)
  const setPinned = useUiStore((state) => state.setTaskWorkbenchTabPinned)
  const setWidth = useUiStore((state) => state.setTaskWorkbenchWidth)
  const workbenchRef = useRef<HTMLElement>(null)
  const tablistRef = useRef<HTMLDivElement>(null)
  const pendingTabFocusRef = useRef<string | null>(null)
  const layoutMode = useTaskWorkbenchLayoutMode(workbenchRef)
  const activeTab = session?.tabs.find((tab) => tab.id === session.activeTabId) ?? null
  const activeTarget = activeTab?.target ?? null

  useEffect(() => {
    if (layoutMode !== 'fullscreen') return
    const workbench = workbenchRef.current
    if (!workbench || workbench.contains(document.activeElement)) return
    workbench.querySelector<HTMLButtonElement>('.task-workbench-back')?.focus()
  }, [layoutMode])

  useLayoutEffect(() => {
    const tabId = pendingTabFocusRef.current
    if (!tabId || activeTab?.id !== tabId) return
    pendingTabFocusRef.current = null
    document.getElementById(taskWorkbenchTabDomId(projection.taskId, tabId))?.focus()
  }, [activeTab?.id, projection.taskId])

  if (!session?.open || !activeTab || !isTaskWorkbenchSidebarTarget(activeTarget)) return null

  function dismissWorkbench() {
    closeWorkbench(projection.taskId)
    onClosed?.()
  }

  function dismissTab(tabId: string) {
    const closesWorkbench = session?.tabs.length === 1
    const closingIndex = session?.tabs.findIndex((tab) => tab.id === tabId) ?? -1
    const remainingTabs = session?.tabs.filter((tab) => tab.id !== tabId) ?? []
    const adjacentTabId =
      session?.activeTabId === tabId && closingIndex >= 0
        ? remainingTabs[Math.min(closingIndex, remainingTabs.length - 1)]?.id
        : undefined
    pendingTabFocusRef.current = adjacentTabId ?? null
    closeTab(projection.taskId, tabId)
    if (closesWorkbench) onClosed?.()
  }

  return (
    <aside
      aria-label={t('workbench.label')}
      className="task-workbench-panel z-30 flex min-h-0 shrink-0 flex-col border-border bg-background shadow-xl"
      data-layout={layoutMode}
      data-target-kind={activeTarget.kind}
      data-testid="task-workbench"
      onKeyDown={(event) => {
        if (event.key === 'Escape') {
          event.stopPropagation()
          dismissWorkbench()
          return
        }
        if (event.key === 'Tab' && layoutMode === 'fullscreen') {
          trapTabKey(event, workbenchRef.current)
        }
      }}
      ref={workbenchRef}
      style={{ '--task-workbench-width': `${width}px` } as CSSProperties}
    >
      <button
        aria-label={t('workbench.resize')}
        className="task-workbench-resizer absolute top-0 bottom-0 left-0 hidden w-2 -translate-x-1/2 cursor-col-resize items-center justify-center text-muted-foreground hover:text-foreground"
        onPointerDown={(event) => {
          event.preventDefault()
          const startX = event.clientX
          const startWidth = width
          const move = (moveEvent: PointerEvent) => {
            setWidth(startWidth + startX - moveEvent.clientX)
          }
          const stop = () => {
            window.removeEventListener('pointermove', move)
            window.removeEventListener('pointerup', stop)
          }
          window.addEventListener('pointermove', move)
          window.addEventListener('pointerup', stop)
        }}
        type="button"
      >
        <GripVertical aria-hidden="true" className="size-3" />
      </button>

      <header className="flex h-11 shrink-0 items-center justify-between gap-3 border-border border-b px-3">
        <div className="flex min-w-0 items-center gap-2">
          <Button
            aria-label={t('workbench.back')}
            className="task-workbench-back -ml-1 hidden size-8 shrink-0"
            onClick={dismissWorkbench}
            size="icon"
            type="button"
            variant="ghost"
          >
            <ArrowLeft aria-hidden="true" className="size-4" />
          </Button>
          <TargetIcon className="size-4 shrink-0 text-muted-foreground" kind={activeTarget.kind} />
          <div className="min-w-0">
            <p className="truncate font-medium text-xs">{projection.title}</p>
            <p className="truncate text-[10px] text-muted-foreground">
              {t(`workbench.targetKind.${activeTarget.kind}`)}
            </p>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          {activeTarget.sourceEventId && onLocateInTimeline ? (
            <Button
              aria-label={t('workbench.locateInConversation')}
              className="size-8"
              onClick={() => onLocateInTimeline(activeTarget.sourceEventId as string)}
              size="icon"
              type="button"
              variant="ghost"
            >
              <MessageSquareReply aria-hidden="true" className="size-4" />
            </Button>
          ) : null}
          <Button
            aria-label={t('workbench.close')}
            className="task-workbench-close size-8"
            onClick={dismissWorkbench}
            size="icon"
            type="button"
            variant="ghost"
          >
            <X aria-hidden="true" className="size-4" />
          </Button>
        </div>
      </header>

      <div className="flex shrink-0 items-stretch border-border border-b">
        <div
          aria-label={t('workbench.tabsLabel')}
          className="flex min-w-0 flex-1 items-stretch gap-0.5 overflow-x-auto px-1.5 pt-1"
          onKeyDown={(event) => {
            if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return
            const buttons = Array.from(
              tablistRef.current?.querySelectorAll<HTMLButtonElement>('[role="tab"]') ?? [],
            )
            const current = buttons.indexOf(document.activeElement as HTMLButtonElement)
            if (current < 0 || buttons.length === 0) return
            event.preventDefault()
            const next =
              event.key === 'Home'
                ? 0
                : event.key === 'End'
                  ? buttons.length - 1
                  : (current + (event.key === 'ArrowRight' ? 1 : -1) + buttons.length) %
                    buttons.length
            buttons[next]?.focus()
            buttons[next]?.click()
          }}
          ref={tablistRef}
          role="tablist"
        >
          {session.tabs.map((tab) => {
            const domId = taskWorkbenchTabDomId(projection.taskId, tab.id)
            return (
              <WorkbenchTab
                active={tab.id === activeTab.id}
                domId={domId}
                key={tab.id}
                onActivate={() => activateTab(projection.taskId, tab.id)}
                onPinnedChange={(pinned) => setPinned(projection.taskId, tab.id, pinned)}
                panelId={`${domId}-panel`}
                tab={tab}
              />
            )
          })}
        </div>
        <div className="flex shrink-0 items-center gap-0.5 border-border border-l px-1">
          <button
            aria-label={activeTab.pinned ? t('workbench.unpinTab') : t('workbench.pinTab')}
            className="grid size-7 place-items-center rounded-md hover:bg-muted"
            onClick={() => setPinned(projection.taskId, activeTab.id, !activeTab.pinned)}
            type="button"
          >
            {activeTab.pinned ? (
              <PinOff aria-hidden="true" className="size-3" />
            ) : (
              <Pin aria-hidden="true" className="size-3" />
            )}
          </button>
          <button
            aria-label={t('workbench.closeTab', { title: activeTab.target.title })}
            className="grid size-7 place-items-center rounded-md hover:bg-muted"
            onClick={() => dismissTab(activeTab.id)}
            type="button"
          >
            <X aria-hidden="true" className="size-3" />
          </button>
        </div>
      </div>

      <div
        aria-label={activeTarget.title}
        aria-labelledby={taskWorkbenchTabDomId(projection.taskId, activeTab.id)}
        className={`min-h-0 flex-1 ${activeTarget.kind === 'browser' ? 'overflow-hidden' : 'overflow-auto'}`}
        id={`${taskWorkbenchTabDomId(projection.taskId, activeTab.id)}-panel`}
        role="tabpanel"
      >
        <WorkbenchContent
          client={client}
          events={events}
          projection={projection}
          snapshotOffset={snapshotOffset}
          target={activeTarget}
          timeline={timeline}
        />
      </div>
    </aside>
  )
}

function WorkbenchTab({
  active,
  domId,
  onActivate,
  onPinnedChange,
  panelId,
  tab,
}: {
  active: boolean
  domId: string
  onActivate: () => void
  onPinnedChange: (pinned: boolean) => void
  panelId: string
  tab: TaskWorkbenchTab
}) {
  return (
    <button
      aria-controls={panelId}
      aria-selected={active}
      className="flex max-w-56 min-w-32 items-center gap-1.5 rounded-t-md border border-transparent border-b-0 px-2 py-2 text-left text-[11px] text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring data-[active=true]:border-border data-[active=true]:bg-surface data-[active=true]:text-foreground"
      data-active={active}
      data-preview={tab.pinned ? undefined : 'true'}
      id={domId}
      onClick={onActivate}
      onDoubleClick={() => {
        if (!tab.pinned) onPinnedChange(true)
      }}
      role="tab"
      tabIndex={active ? 0 : -1}
      type="button"
    >
      <TargetIcon className="size-3.5 shrink-0" kind={tab.target.kind} />
      <span className={tab.pinned ? 'truncate' : 'truncate italic'}>{tab.target.title}</span>
    </button>
  )
}

function WorkbenchContent({
  client,
  events,
  projection,
  snapshotOffset,
  target,
  timeline,
}: {
  client: Pick<DaemonClient, 'loadTaskEvents' | 'readBlob' | 'request'>
  events: TaskEventEnvelope[]
  projection: TaskProjection
  snapshotOffset: number
  target: TaskWorkbenchTarget
  timeline: TimelineItemProjection[]
}) {
  const { t } = useTranslation('tasks')
  if (target.kind === 'diff' || target.kind === 'command') {
    return <LegacyArtifactPanel client={client} target={target} />
  }
  if (
    target.kind === 'source' &&
    !target.blobId &&
    !target.artifact?.previewBlobId &&
    !target.artifact?.preview
  ) {
    return (
      <SourcesPanel
        events={events}
        loading={false}
        missing={false}
        text={null}
        timeline={timeline}
      />
    )
  }
  if (target.kind === 'file' || target.kind === 'artifact' || target.kind === 'source') {
    if (target.blobId || target.artifact?.previewBlobId || target.artifact?.preview) {
      return (
        <ArtifactRenderer
          artifact={artifactDescriptorForTarget(target)}
          loader={client.readBlob}
          surface="workbench"
        />
      )
    }
    return (
      <ArtifactText
        empty={t('workbench.empty.artifact')}
        loading={false}
        missing={false}
        text={null}
      />
    )
  }
  if (target.kind === 'subagent') {
    return <SubagentsPanel events={events} subagents={projection.subagents ?? []} target={target} />
  }
  if (target.kind === 'browser') {
    return <BrowserPanel client={client} taskId={projection.taskId} />
  }
  if (target.kind === 'environment') {
    return (
      <EnvironmentPanel
        events={events}
        target={target}
        timeline={timeline}
        workspace={projection.workspace}
      />
    )
  }
  return (
    <AuditPanel
      client={client}
      liveEvents={events}
      snapshotOffset={snapshotOffset}
      taskId={projection.taskId}
      target={target}
      timeline={timeline}
    />
  )
}

function LegacyArtifactPanel({
  client,
  target,
}: {
  client: Pick<DaemonClient, 'readBlob'>
  target: TaskWorkbenchTarget
}) {
  const artifact = useArtifactResource(
    artifactDescriptorForTarget(target),
    client.readBlob,
    'workbench',
  )
  const props = {
    error: artifact.error,
    loading: artifact.loading,
    missing: artifact.missing,
    onRetry: artifact.retry,
    text: artifact.text,
  }
  return target.kind === 'diff' ? <DiffPanel {...props} /> : <CommandPanel {...props} />
}

function artifactDescriptorForTarget(target: TaskWorkbenchTarget): ArtifactDescriptor {
  return {
    artifactId: target.artifact?.artifactId,
    artifactKind: target.artifact?.artifactKind ?? target.kind,
    blobId: target.blobId,
    format: target.artifact?.format,
    mediaType: target.artifact?.mediaType ?? 'application/octet-stream',
    presentation: {
      preferredSurface: target.artifact?.preferredSurface,
      previewBlobId: target.artifact?.previewBlobId,
    },
    preview: target.artifact?.preview,
    size: target.artifact?.size,
    title: target.title,
  }
}

function TargetIcon({ className, kind }: { className?: string; kind: TaskWorkbenchTargetKind }) {
  if (kind === 'browser') return <Globe2 aria-hidden="true" className={className} />
  if (kind === 'diff') return <FileDiff aria-hidden="true" className={className} />
  if (kind === 'command') return <SquareTerminal aria-hidden="true" className={className} />
  if (kind === 'source') return <ImageIcon aria-hidden="true" className={className} />
  if (kind === 'subagent') return <Bot aria-hidden="true" className={className} />
  if (kind === 'environment') return <FolderGit2 aria-hidden="true" className={className} />
  if (kind === 'audit') return <ListTree aria-hidden="true" className={className} />
  return <FileText aria-hidden="true" className={className} />
}

type TaskWorkbenchLayoutMode = 'docked' | 'fullscreen' | 'overlay'

function useTaskWorkbenchLayoutMode(workbenchRef: React.RefObject<HTMLElement | null>) {
  const [mode, setMode] = useState<TaskWorkbenchLayoutMode>('docked')

  useEffect(() => {
    const container = workbenchRef.current?.closest<HTMLElement>('.task-workspace-container')
    if (!container) return
    const update = () => setMode(layoutModeForWidth(container.getBoundingClientRect().width))
    update()
    if (typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(update)
    observer.observe(container)
    return () => observer.disconnect()
  }, [workbenchRef])

  return mode
}

function layoutModeForWidth(width: number): TaskWorkbenchLayoutMode {
  if (width < 720) return 'fullscreen'
  if (width < 1040) return 'overlay'
  return 'docked'
}

function taskWorkbenchTabDomId(taskId: string, tabId: string) {
  return `task-workbench-tab-${encodeURIComponent(taskId)}-${encodeURIComponent(tabId)}`
}

function trapTabKey(event: React.KeyboardEvent<HTMLElement>, container: HTMLElement | null) {
  if (!container) return
  const focusable = Array.from(
    container.querySelectorAll<HTMLElement>(
      'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
    ),
  ).filter((element) => !element.hasAttribute('hidden') && element.offsetParent !== null)
  const first = focusable.at(0)
  const last = focusable.at(-1)
  if (!first || !last) return
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault()
    last.focus()
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault()
    first.focus()
  }
}
