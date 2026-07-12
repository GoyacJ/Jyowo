import { useVirtualizer } from '@tanstack/react-virtual'
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type {
  TaskEventEnvelope,
  TimelineItemProjection,
  TypedUlid,
} from '@/generated/daemon-protocol'
import type { DaemonClient, TaskEventPage } from '@/shared/daemon/client'
import { Button } from '@/shared/ui/button'

import { ProjectionList } from './EnvironmentPanel'

const auditPageSize = 16
const estimatedEventHeightPx = 42

type AuditPanelProps = {
  client: Pick<DaemonClient, 'loadTaskEvents'>
  liveEvents: TaskEventEnvelope[]
  snapshotOffset: number
  taskId: TypedUlid
  timeline: TimelineItemProjection[]
}

export function AuditPanel(props: AuditPanelProps) {
  return <ScopedAuditPanel key={`${props.taskId}:${props.snapshotOffset}`} {...props} />
}

function ScopedAuditPanel({ client, liveEvents, taskId, timeline }: AuditPanelProps) {
  const { t } = useTranslation('tasks')
  const [cursor, setCursor] = useState<number | undefined>()
  const [newerCursors, setNewerCursors] = useState<Array<number | undefined>>([])
  const [page, setPage] = useState<TaskEventPage | null>(null)
  const [loading, setLoading] = useState(true)
  const [failed, setFailed] = useState(false)

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    setFailed(false)
    void client
      .loadTaskEvents(taskId, cursor)
      .then((nextPage) => {
        if (!cancelled) setPage(nextPage)
      })
      .catch(() => {
        if (!cancelled) setFailed(true)
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [client.loadTaskEvents, cursor, taskId])

  const combinedEvents = useMemo(() => {
    const combined =
      cursor === undefined ? [...(page?.events ?? []), ...liveEvents] : (page?.events ?? [])
    return [...new Map(combined.map((event) => [event.eventId, event])).values()].sort(
      (left, right) => right.globalOffset - left.globalOffset,
    )
  }, [cursor, liveEvents, page?.events])
  const events = combinedEvents.slice(0, auditPageSize)
  const olderCursor =
    cursor === undefined
      ? combinedEvents.length > auditPageSize || page?.nextBeforeOffset != null
        ? (events.at(-1)?.globalOffset ?? null)
        : null
      : (page?.nextBeforeOffset ?? null)

  if (loading && !page) {
    return <p className="p-4 text-muted-foreground text-sm">{t('workbench.audit.loading')}</p>
  }
  if (failed) {
    return <p className="p-4 text-destructive text-sm">{t('workbench.audit.unavailable')}</p>
  }
  if (events.length === 0) {
    const projectedEvents = timeline.filter((item) =>
      ['compaction', 'error', 'notice', 'permission', 'tool_activity'].includes(item.kind),
    )
    if (projectedEvents.length > 0) return <ProjectionList items={projectedEvents} />
    return <p className="p-4 text-muted-foreground text-sm">{t('workbench.empty.audit')}</p>
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <VirtualEventList events={events} />
      <div className="flex shrink-0 items-center justify-between border-border border-t p-2">
        {newerCursors.length > 0 ? (
          <Button
            disabled={loading}
            onClick={() => {
              const history = [...newerCursors]
              setCursor(history.pop())
              setNewerCursors(history)
            }}
            size="sm"
            type="button"
            variant="ghost"
          >
            {t('workbench.audit.newer')}
          </Button>
        ) : (
          <span />
        )}
        {olderCursor != null ? (
          <Button
            disabled={loading}
            onClick={() => {
              setNewerCursors((history) => [...history, cursor])
              setCursor(olderCursor)
            }}
            size="sm"
            type="button"
            variant="ghost"
          >
            {t('workbench.audit.older')}
          </Button>
        ) : null}
      </div>
    </div>
  )
}

function VirtualEventList({ events }: { events: TaskEventEnvelope[] }) {
  const viewportRef = useRef<HTMLDivElement>(null)
  const virtualizer = useVirtualizer({
    count: events.length,
    estimateSize: () => estimatedEventHeightPx,
    getItemKey: (index) => events[index]?.eventId ?? index,
    getScrollElement: () => viewportRef.current,
    initialRect: { height: 480, width: 360 },
    overscan: 4,
  })
  const virtualRows = virtualizer.getVirtualItems()
  const rows =
    virtualRows.length > 0
      ? virtualRows.map((row) => ({ index: row.index, start: row.start }))
      : events.map((_, index) => ({ index, start: index * estimatedEventHeightPx }))

  return (
    <div className="min-h-0 flex-1 overflow-auto" ref={viewportRef}>
      <ol className="relative" style={{ height: `${virtualizer.getTotalSize()}px` }}>
        {rows.map((row) => {
          const event = events[row.index]
          if (!event) return null
          return (
            <li
              className="absolute top-0 left-0 grid w-full grid-cols-[1fr_auto] gap-3 border-border/70 border-b px-4 py-3"
              data-audit-event="true"
              data-index={row.index}
              key={event.eventId}
              ref={virtualizer.measureElement}
              style={{ transform: `translateY(${row.start}px)` }}
            >
              <span className="text-sm">{event.eventType}</span>
              <span className="font-mono text-[11px] text-muted-foreground">
                #{event.globalOffset}
              </span>
            </li>
          )
        })}
      </ol>
    </div>
  )
}
