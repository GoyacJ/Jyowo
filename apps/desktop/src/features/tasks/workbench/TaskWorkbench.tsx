import { Columns3, PanelRight, X } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'

import type {
  TaskEventEnvelope,
  TaskProjection,
  TimelineItemProjection,
} from '@/generated/daemon-protocol'
import type { DaemonClient } from '@/shared/daemon/client'
import { useUiStore } from '@/shared/state/ui-store'
import type { TaskWorkbenchPanel, TaskWorkbenchSelection } from '@/shared/state/workbench-selection'
import { Button } from '@/shared/ui/button'

import { AuditPanel } from './AuditPanel'
import { CommandPanel } from './CommandPanel'
import { DiffPanel } from './DiffPanel'
import { EnvironmentPanel } from './EnvironmentPanel'
import { SourcesPanel } from './SourcesPanel'
import { SubagentsPanel } from './SubagentsPanel'

const tabs: Array<{ label: string; panel: TaskWorkbenchPanel }> = [
  { label: 'Changes', panel: 'changes' },
  { label: 'Commands', panel: 'commands' },
  { label: 'Agents', panel: 'agents' },
  { label: 'Environment', panel: 'environment' },
  { label: 'Sources', panel: 'sources' },
  { label: 'Audit', panel: 'audit' },
]

export function TaskWorkbench({
  client,
  events,
  projection,
  timeline = [],
}: {
  client: Pick<DaemonClient, 'readBlob'>
  events: TaskEventEnvelope[]
  projection: TaskProjection
  timeline?: TimelineItemProjection[]
}) {
  const mode = useUiStore((state) => state.taskWorkbenchMode)
  const selection = useUiStore((state) => state.taskWorkbenchSelection)
  const setMode = useUiStore((state) => state.setTaskWorkbenchMode)
  const setSelection = useUiStore((state) => state.setTaskWorkbenchSelection)
  const activePanel = selection?.panel ?? 'changes'
  const [artifact, setArtifact] = useState<{
    blobId?: string
    loading: boolean
    missing: boolean
    text: string | null
  }>({ loading: false, missing: false, text: null })
  const tablistRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const blobId = selection?.blobId
    if (!blobId || !['changes', 'commands', 'sources'].includes(activePanel)) {
      setArtifact({ blobId, loading: false, missing: false, text: null })
      return
    }
    let cancelled = false
    setArtifact({ blobId, loading: true, missing: false, text: null })
    void client
      .readBlob(blobId)
      .then((blob) => {
        if (cancelled) return
        setArtifact({
          blobId,
          loading: false,
          missing: blob.missing || blob.bytes === null,
          text: blob.bytes ? new TextDecoder().decode(blob.bytes) : null,
        })
      })
      .catch(() => {
        if (!cancelled) setArtifact({ blobId, loading: false, missing: true, text: null })
      })
    return () => {
      cancelled = true
    }
  }, [activePanel, client.readBlob, selection?.blobId])

  if (mode === 'closed') return null

  function selectPanel(panel: TaskWorkbenchPanel) {
    setSelection({
      blobId: panel === activePanel ? selection?.blobId : undefined,
      eventId: selection?.eventId ?? projection.taskId,
      panel,
      segmentId: selection?.segmentId,
      taskId: projection.taskId,
    })
  }

  return (
    <aside
      aria-label="Task workbench"
      className="task-workbench-panel static z-auto flex h-full min-h-[360px] w-full shrink-0 flex-col border-border border-t bg-background shadow-none"
      data-mode={mode}
    >
      <header className="flex h-11 shrink-0 items-center justify-between border-border border-b px-3">
        <span className="font-medium text-xs">Workbench</span>
        <div className="flex items-center gap-1">
          <Button
            aria-label={
              mode === 'collaboration' ? 'Use inspector width' : 'Use collaboration width'
            }
            className="size-7"
            onClick={() => setMode(mode === 'collaboration' ? 'inspector' : 'collaboration')}
            size="icon"
            type="button"
            variant="ghost"
          >
            {mode === 'collaboration' ? (
              <PanelRight aria-hidden="true" className="size-4" />
            ) : (
              <Columns3 aria-hidden="true" className="size-4" />
            )}
          </Button>
          <Button
            aria-label="Close task workbench"
            className="size-7"
            onClick={() => {
              setSelection(null)
              setMode('closed')
            }}
            size="icon"
            type="button"
            variant="ghost"
          >
            <X aria-hidden="true" className="size-4" />
          </Button>
        </div>
      </header>
      <div
        aria-label="Task workbench panels"
        className="grid grid-cols-6 overflow-hidden border-border border-b px-2"
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
        {tabs.map((tab) => (
          <button
            aria-controls={`task-workbench-panel-${tab.panel}`}
            aria-selected={activePanel === tab.panel}
            className="min-w-0 border-transparent border-b-2 px-1.5 py-2 text-[11px] text-muted-foreground aria-selected:border-foreground aria-selected:text-foreground"
            id={`task-workbench-tab-${tab.panel}`}
            key={tab.panel}
            onClick={() => selectPanel(tab.panel)}
            role="tab"
            tabIndex={activePanel === tab.panel ? 0 : -1}
            type="button"
          >
            {tab.label}
          </button>
        ))}
      </div>
      {selection ? <SelectionIdentity selection={selection} /> : null}
      <div
        aria-labelledby={`task-workbench-tab-${activePanel}`}
        className="min-h-0 flex-1 overflow-auto"
        id={`task-workbench-panel-${activePanel}`}
        role="tabpanel"
      >
        {activePanel === 'changes' ? <DiffPanel {...artifact} /> : null}
        {activePanel === 'commands' ? <CommandPanel {...artifact} /> : null}
        {activePanel === 'agents' ? (
          <SubagentsPanel subagents={projection.subagents ?? []} />
        ) : null}
        {activePanel === 'environment' ? (
          <EnvironmentPanel events={events} timeline={timeline} />
        ) : null}
        {activePanel === 'sources' ? (
          <SourcesPanel events={events} timeline={timeline} {...artifact} />
        ) : null}
        {activePanel === 'audit' ? <AuditPanel events={events} timeline={timeline} /> : null}
      </div>
    </aside>
  )
}

function SelectionIdentity({ selection }: { selection: TaskWorkbenchSelection }) {
  return (
    <dl className="grid shrink-0 grid-cols-[auto_minmax(0,1fr)] gap-x-2 gap-y-1 border-border border-b px-3 py-2 font-mono text-[10px] text-muted-foreground">
      <dt>Task</dt>
      <dd className="truncate">{selection.taskId}</dd>
      {selection.segmentId ? (
        <>
          <dt>Segment</dt>
          <dd className="truncate">{selection.segmentId}</dd>
        </>
      ) : null}
      <dt>Event</dt>
      <dd className="truncate">{selection.eventId}</dd>
    </dl>
  )
}
