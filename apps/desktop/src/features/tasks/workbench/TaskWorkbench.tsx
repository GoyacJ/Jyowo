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
  const [blobRetry, setBlobRetry] = useState(0)
  const [artifact, setArtifact] = useState<{
    blobId?: string
    error: boolean
    imageUrl: string | null
    loading: boolean
    mediaType: string | null
    missing: boolean
    size: number | null
    text: string | null
  }>({
    error: false,
    imageUrl: null,
    loading: false,
    mediaType: null,
    missing: false,
    size: null,
    text: null,
  })

  useEffect(() => {
    const blobId = activeTarget?.blobId
    if (!blobId || !activeTarget || !blobKinds.has(activeTarget.kind)) {
      setArtifact({
        blobId,
        error: false,
        imageUrl: null,
        loading: false,
        mediaType: null,
        missing: false,
        size: null,
        text: null,
      })
      return
    }
    let cancelled = false
    let imageUrl: string | null = null
    setArtifact({
      blobId,
      error: false,
      imageUrl: null,
      loading: true,
      mediaType: null,
      missing: false,
      size: null,
      text: null,
    })
    void client
      .readBlob(blobId)
      .then((blob) => {
        if (cancelled) return
        const bytes = blob.bytes
        const mediaType = blob.mediaType || 'application/octet-stream'
        if (bytes && mediaType.startsWith('image/')) {
          imageUrl = URL.createObjectURL(new Blob([Uint8Array.from(bytes)], { type: mediaType }))
        }
        setArtifact({
          blobId,
          error: false,
          imageUrl,
          loading: false,
          mediaType,
          missing: blob.missing || blob.bytes === null,
          size: blob.size,
          text: bytes && isTextMediaType(mediaType) ? new TextDecoder().decode(bytes) : null,
        })
      })
      .catch(() => {
        if (!cancelled) {
          setArtifact({
            blobId,
            error: true,
            imageUrl: null,
            loading: false,
            mediaType: null,
            missing: false,
            size: null,
            text: null,
          })
        }
      })
    return () => {
      cancelled = true
      if (imageUrl) URL.revokeObjectURL(imageUrl)
    }
  }, [activeTarget, blobRetry, client.readBlob])

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
            className="task-workbench-back -ml-1 hidden size-7 shrink-0"
            onClick={dismissWorkbench}
            size="icon"
            type="button"
            variant="ghost"
          >
            <ArrowLeft aria-hidden="true" className="size-4" />
          </Button>
          <TargetIcon className="size-4 shrink-0 text-muted-foreground" kind={activeTarget.kind} />
          <div className="min-w-0">
            <p className="truncate font-medium text-xs">{activeTarget.title}</p>
            <p className="truncate text-[10px] text-muted-foreground">
              {t(`workbench.targetKind.${activeTarget.kind}`)} · {projection.title}
            </p>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          {activeTarget.sourceEventId && onLocateInTimeline ? (
            <Button
              aria-label={t('workbench.locateInConversation')}
              className="size-7"
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
            className="task-workbench-close size-7"
            onClick={dismissWorkbench}
            size="icon"
            type="button"
            variant="ghost"
          >
            <X aria-hidden="true" className="size-4" />
          </Button>
        </div>
      </header>

      <div
        aria-label={t('workbench.tabsLabel')}
        className="flex shrink-0 items-stretch gap-0.5 overflow-x-auto border-border border-b px-1.5 pt-1"
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
              onClose={() => dismissTab(tab.id)}
              onPinnedChange={(pinned) => setPinned(projection.taskId, tab.id, pinned)}
              panelId={`${domId}-panel`}
              tab={tab}
            />
          )
        })}
      </div>

      <div
        aria-label={activeTarget.title}
        aria-labelledby={taskWorkbenchTabDomId(projection.taskId, activeTab.id)}
        className={`min-h-0 flex-1 ${activeTarget.kind === 'browser' ? 'overflow-hidden' : 'overflow-auto'}`}
        id={`${taskWorkbenchTabDomId(projection.taskId, activeTab.id)}-panel`}
        role="tabpanel"
      >
        <WorkbenchContent
          artifact={artifact}
          client={client}
          events={events}
          projection={projection}
          snapshotOffset={snapshotOffset}
          target={activeTarget}
          timeline={timeline}
          onRetryArtifact={() => setBlobRetry((value) => value + 1)}
        />
      </div>
    </aside>
  )
}

function WorkbenchTab({
  active,
  domId,
  onActivate,
  onClose,
  onPinnedChange,
  panelId,
  tab,
}: {
  active: boolean
  domId: string
  onActivate: () => void
  onClose: () => void
  onPinnedChange: (pinned: boolean) => void
  panelId: string
  tab: TaskWorkbenchTab
}) {
  const { t } = useTranslation('tasks')
  return (
    <div
      className="group flex max-w-56 min-w-32 items-center rounded-t-md border border-transparent border-b-0 text-muted-foreground data-[active=true]:border-border data-[active=true]:bg-surface data-[active=true]:text-foreground"
      data-active={active}
      data-preview={tab.pinned ? undefined : 'true'}
    >
      <button
        aria-controls={panelId}
        aria-selected={active}
        className="flex min-w-0 flex-1 items-center gap-1.5 px-2 py-2 text-left text-[11px] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
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
      <button
        aria-label={tab.pinned ? t('workbench.unpinTab') : t('workbench.pinTab')}
        className="rounded p-1 opacity-0 hover:bg-muted group-hover:opacity-100 group-focus-within:opacity-100"
        onClick={() => onPinnedChange(!tab.pinned)}
        type="button"
      >
        {tab.pinned ? (
          <PinOff aria-hidden="true" className="size-3" />
        ) : (
          <Pin aria-hidden="true" className="size-3" />
        )}
      </button>
      <button
        aria-label={t('workbench.closeTab', { title: tab.target.title })}
        className="mr-1 rounded p-1 hover:bg-muted"
        onClick={onClose}
        type="button"
      >
        <X aria-hidden="true" className="size-3" />
      </button>
    </div>
  )
}

function WorkbenchContent({
  artifact,
  client,
  events,
  onRetryArtifact,
  projection,
  snapshotOffset,
  target,
  timeline,
}: {
  artifact: {
    error: boolean
    imageUrl: string | null
    loading: boolean
    mediaType: string | null
    missing: boolean
    size: number | null
    text: string | null
  }
  client: Pick<DaemonClient, 'loadTaskEvents' | 'request'>
  events: TaskEventEnvelope[]
  onRetryArtifact: () => void
  projection: TaskProjection
  snapshotOffset: number
  target: TaskWorkbenchTarget
  timeline: TimelineItemProjection[]
}) {
  const { t } = useTranslation('tasks')
  const unsupported =
    !artifact.error &&
    !artifact.loading &&
    !artifact.missing &&
    artifact.mediaType !== null &&
    !isTextMediaType(artifact.mediaType) &&
    !artifact.mediaType.startsWith('image/')
  if (unsupported) {
    return (
      <UnsupportedArtifact
        mediaType={artifact.mediaType as string}
        size={artifact.size}
        title={target.title}
      />
    )
  }
  if (artifact.imageUrl) {
    return (
      <ArtifactImage mediaType={artifact.mediaType} src={artifact.imageUrl} title={target.title} />
    )
  }
  if (target.kind === 'diff') return <DiffPanel {...artifact} onRetry={onRetryArtifact} />
  if (target.kind === 'command') return <CommandPanel {...artifact} onRetry={onRetryArtifact} />
  if (target.kind === 'source') {
    return (
      <SourcesPanel events={events} onRetry={onRetryArtifact} timeline={timeline} {...artifact} />
    )
  }
  if (target.kind === 'file' || target.kind === 'artifact') {
    return (
      <ArtifactText
        empty={t('workbench.empty.artifact')}
        error={artifact.error}
        loading={artifact.loading}
        missing={artifact.missing}
        onRetry={onRetryArtifact}
        text={artifact.text}
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

function ArtifactImage({
  mediaType,
  src,
  title,
}: {
  mediaType: string | null
  src: string
  title: string
}) {
  return (
    <figure className="flex min-h-full flex-col items-center justify-center gap-3 bg-muted/20 p-4">
      <img alt={title} className="max-h-full max-w-full object-contain" src={src} />
      <figcaption className="font-mono text-[11px] text-muted-foreground">{mediaType}</figcaption>
    </figure>
  )
}

function UnsupportedArtifact({
  mediaType,
  size,
  title,
}: {
  mediaType: string
  size: number | null
  title: string
}) {
  const { t } = useTranslation('tasks')
  return (
    <div className="flex min-h-48 flex-col items-center justify-center gap-2 px-6 text-center">
      <FileText aria-hidden="true" className="size-6 text-muted-foreground" />
      <p className="text-sm">{t('workbench.artifact.unsupported')}</p>
      <p className="max-w-full truncate font-mono text-[11px] text-muted-foreground">{title}</p>
      <p className="font-mono text-[11px] text-muted-foreground">
        {mediaType}
        {size === null ? '' : ` · ${t('workbench.artifact.bytes', { count: size })}`}
      </p>
    </div>
  )
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

const blobKinds = new Set<TaskWorkbenchTargetKind>([
  'artifact',
  'command',
  'diff',
  'file',
  'source',
])

function isTextMediaType(mediaType: string) {
  const normalized = mediaType.toLowerCase().split(';', 1)[0]?.trim() ?? ''
  return (
    normalized.startsWith('text/') ||
    normalized.endsWith('+json') ||
    normalized.endsWith('+xml') ||
    [
      'application/javascript',
      'application/json',
      'application/toml',
      'application/xml',
      'application/x-httpd-php',
      'application/x-sh',
      'application/x-yaml',
    ].includes(normalized)
  )
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
