import { useVirtualizer } from '@tanstack/react-virtual'
import type { TFunction } from 'i18next'
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { RawJsonView } from '@/features/activity/RawJsonView'
import type {
  TaskEventEnvelope,
  TimelineItemProjection,
  TypedUlid,
} from '@/generated/daemon-protocol'
import type { DaemonClient, TaskEventPage } from '@/shared/daemon/client'
import type { TaskWorkbenchTarget } from '@/shared/state/workbench-selection'
import { Button } from '@/shared/ui/button'

import { timelineSummary } from '../timeline/timeline-summary'
import { ProjectionList } from './EnvironmentPanel'

const auditPageSize = 16
const estimatedEventHeightPx = 42

type AuditPanelProps = {
  client: Pick<DaemonClient, 'loadTaskEvents'>
  liveEvents: TaskEventEnvelope[]
  snapshotOffset: number
  taskId: TypedUlid
  target?: TaskWorkbenchTarget
  timeline: TimelineItemProjection[]
}

export function AuditPanel(props: AuditPanelProps) {
  return (
    <ScopedAuditPanel
      key={`${props.taskId}:${props.snapshotOffset}:${props.target?.resourceId ?? 'all'}`}
      {...props}
    />
  )
}

function ScopedAuditPanel({ client, liveEvents, target, taskId, timeline }: AuditPanelProps) {
  const { i18n, t } = useTranslation('tasks')
  const [cursor, setCursor] = useState<number | undefined>()
  const [newerCursors, setNewerCursors] = useState<Array<number | undefined>>([])
  const [page, setPage] = useState<TaskEventPage | null>(null)
  const [loading, setLoading] = useState(true)
  const [failed, setFailed] = useState(false)
  const [retry, setRetry] = useState(0)
  const [selectedEventId, setSelectedEventId] = useState<string | null>(null)

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
  }, [client.loadTaskEvents, cursor, retry, taskId])

  const combinedEvents = useMemo(() => {
    const combined =
      cursor === undefined ? [...(page?.events ?? []), ...liveEvents] : (page?.events ?? [])
    return [...new Map(combined.map((event) => [event.eventId, event])).values()].sort(
      (left, right) => right.globalOffset - left.globalOffset,
    )
  }, [cursor, liveEvents, page?.events])
  const selectedEvents = selectAuditEvents(combinedEvents, target)
  const events = selectedEvents.slice(0, auditPageSize)
  const olderCursor =
    target && target.resourceId !== 'all'
      ? null
      : cursor === undefined
        ? combinedEvents.length > auditPageSize || page?.nextBeforeOffset != null
          ? (events.at(-1)?.globalOffset ?? null)
          : null
        : (page?.nextBeforeOffset ?? null)

  if (loading && !page) {
    return <p className="p-4 text-muted-foreground text-sm">{t('workbench.audit.loading')}</p>
  }
  if (failed) {
    return (
      <div className="flex min-h-48 flex-col items-center justify-center gap-3 px-6 text-center text-destructive text-sm">
        <span>{t('workbench.audit.unavailable')}</span>
        <Button
          onClick={() => setRetry((value) => value + 1)}
          size="sm"
          type="button"
          variant="outline"
        >
          {t('workbench.artifact.retry')}
        </Button>
      </div>
    )
  }
  if (events.length === 0) {
    const projectedEvents = timeline.filter(
      (item) =>
        ['compaction', 'error', 'notice', 'permission', 'tool_activity'].includes(item.kind) &&
        (!target ||
          target.resourceId === 'all' ||
          item.id === target.resourceId ||
          item.id === target.sourceEventId),
    )
    if (projectedEvents.length > 0) return <ProjectionList items={projectedEvents} />
    return <p className="p-4 text-muted-foreground text-sm">{t('workbench.empty.audit')}</p>
  }

  const selectedEvent = events.find((event) => event.eventId === selectedEventId)
  const hasSpecificTarget = Boolean(target && target.resourceId !== 'all')
  if (selectedEvent || (hasSpecificTarget && events.length === 1)) {
    const event = selectedEvent ?? events[0]
    if (event) {
      return (
        <AuditEventDetails
          event={event}
          locale={i18n.resolvedLanguage ?? i18n.language}
          onBack={hasSpecificTarget ? undefined : () => setSelectedEventId(null)}
          timeline={timeline}
        />
      )
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <VirtualEventList events={events} onSelect={setSelectedEventId} timeline={timeline} />
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

function selectAuditEvents(events: TaskEventEnvelope[], target: TaskWorkbenchTarget | undefined) {
  if (!target || target.resourceId === 'all') return events
  return events.filter(
    (event) => event.eventId === target.resourceId || event.eventId === target.sourceEventId,
  )
}

function VirtualEventList({
  events,
  onSelect,
  timeline,
}: {
  events: TaskEventEnvelope[]
  onSelect: (eventId: string) => void
  timeline: TimelineItemProjection[]
}) {
  const { t } = useTranslation('tasks')
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
          const summary = auditEventSummary(event, timeline, t)
          return (
            <li
              className="absolute top-0 left-0 w-full border-border/70 border-b"
              data-audit-event="true"
              data-index={row.index}
              key={event.eventId}
              ref={virtualizer.measureElement}
              style={{ transform: `translateY(${row.start}px)` }}
            >
              <button
                className="grid w-full grid-cols-[minmax(0,1fr)_auto] gap-3 px-4 py-3 text-left hover:bg-muted/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
                onClick={() => onSelect(event.eventId)}
                type="button"
              >
                <span className="min-w-0">
                  <span className="block truncate text-sm">{summary}</span>
                  {summary !== event.eventType ? (
                    <span className="block truncate font-mono text-[11px] text-muted-foreground">
                      {event.eventType}
                    </span>
                  ) : null}
                </span>
                <span className="font-mono text-[11px] text-muted-foreground">
                  #{event.globalOffset}
                </span>
              </button>
            </li>
          )
        })}
      </ol>
    </div>
  )
}

function AuditEventDetails({
  event,
  locale,
  onBack,
  timeline,
}: {
  event: TaskEventEnvelope
  locale: string
  onBack?: () => void
  timeline: TimelineItemProjection[]
}) {
  const { t } = useTranslation('tasks')
  const payload = isRecord(event.payload) ? event.payload : { value: event.payload }
  return (
    <div className="h-full overflow-y-auto p-4">
      {onBack ? (
        <Button className="mb-3" onClick={onBack} size="sm" type="button" variant="ghost">
          {t('workbench.audit.back')}
        </Button>
      ) : null}
      <header className="space-y-1 border-border/70 border-b pb-4">
        <h2 className="break-words font-medium text-sm">{auditEventSummary(event, timeline, t)}</h2>
        <p className="break-all font-mono text-[11px] text-muted-foreground">{event.eventType}</p>
      </header>
      <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-3 py-4 text-sm">
        <dt className="text-muted-foreground">{t('workbench.audit.status')}</dt>
        <dd>{t(`workbench.audit.eventStatus.${auditEventStatus(event)}`)}</dd>
        <dt className="text-muted-foreground">{t('workbench.audit.time')}</dt>
        <dd>{formatAuditTime(event.recordedAt, locale)}</dd>
        <dt className="text-muted-foreground">{t('workbench.audit.source')}</dt>
        <dd className="break-all">{formatAuditSource(event)}</dd>
        <dt className="text-muted-foreground">{t('timeline.offset')}</dt>
        <dd className="font-mono">#{event.globalOffset}</dd>
      </dl>
      <RawJsonView rawJson={{ payload }} />
    </div>
  )
}

function auditEventSummary(
  event: TaskEventEnvelope,
  timeline: TimelineItemProjection[],
  t: TFunction<'tasks'>,
) {
  const item = timeline.find(
    (candidate) => candidate.id === event.eventId || candidate.globalOffset === event.globalOffset,
  )
  return item ? timelineSummary(item, t) : event.eventType
}

function auditEventStatus(event: TaskEventEnvelope) {
  const value = event.eventType.toLowerCase()
  if (/(failed|error|denied|timed_out|blocked)/.test(value)) return 'failed'
  if (/(started|starting|requested|waiting|preparing)/.test(value)) return 'running'
  if (/(completed|ended|resolved|released|acquired|created)/.test(value)) return 'complete'
  return 'recorded'
}

function formatAuditTime(value: string, locale: string) {
  const date = new Date(value)
  return Number.isNaN(date.getTime())
    ? value
    : new Intl.DateTimeFormat(locale, {
        dateStyle: 'medium',
        timeStyle: 'medium',
      }).format(date)
}

function formatAuditSource(event: TaskEventEnvelope) {
  const identity = event.source.actorId ?? event.source.clientId
  return identity ? `${event.source.kind} · ${identity}` : event.source.kind
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}
