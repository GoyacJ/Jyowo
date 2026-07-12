import type { RunProjection, TimelineItemProjection } from '@/generated/daemon-protocol'

import { TimelineEvent, TimelineItem } from './TimelineEvent'

export function RunSegment({
  items,
  onSelectItem,
  run,
  segmentId,
  showHeader = true,
  statusItems = items,
}: {
  items: TimelineItemProjection[]
  onSelectItem?: (item: TimelineItemProjection) => void
  run?: RunProjection | null
  segmentId: string
  showHeader?: boolean
  statusItems?: TimelineItemProjection[]
}) {
  const status = run?.segmentId === segmentId ? projectedStatus(run) : inferStatus(statusItems)
  const duration = run?.segmentId === segmentId ? formatDuration(run) : null
  const content = <div className="space-y-4">{renderItems(items, onSelectItem)}</div>

  if (!showHeader) {
    return (
      <div className="space-y-5" data-run-segment={segmentId}>
        {content}
      </div>
    )
  }

  return (
    <section aria-label={`Run ${status}`} className="space-y-5" data-run-segment={segmentId}>
      <div className="flex items-center gap-3 text-muted-foreground text-xs">
        <span className="font-medium capitalize text-foreground">{status.replace('_', ' ')}</span>
        {duration ? <span>{duration}</span> : null}
        <span aria-hidden="true" className="h-px flex-1 bg-border" />
      </div>
      {content}
    </section>
  )
}

function projectedStatus(run: RunProjection) {
  switch (run.terminalReason) {
    case 'cancelled':
      return 'cancelled'
    case 'superseded':
      return 'superseded'
    case 'forced_interruption':
    case 'interrupted_by_restart':
      return 'interrupted'
    case 'failed':
      return 'failed'
    case 'completed':
      return 'completed'
    default:
      return run.state
  }
}

function renderItems(
  items: TimelineItemProjection[],
  onSelectItem?: (item: TimelineItemProjection) => void,
) {
  const rendered: React.ReactNode[] = []
  let index = 0

  while (index < items.length) {
    const item = items[index]
    if (!item) break
    if (item.kind !== 'assistant_text') {
      rendered.push(<TimelineEvent item={item} key={item.id} onSelect={onSelectItem} />)
      index += 1
      continue
    }

    const narrative: TimelineItemProjection[] = []
    while (
      items[index]?.kind === 'assistant_text' &&
      items[index]?.runSegmentId === item.runSegmentId &&
      items[index]?.semanticGroupId === item.semanticGroupId
    ) {
      narrative.push(items[index] as TimelineItemProjection)
      index += 1
    }
    rendered.push(
      <div
        className="whitespace-pre-wrap text-[15px] leading-7 text-foreground"
        data-narrative="true"
        key={narrative[0]?.id}
      >
        {narrative.map((entry) => (
          <TimelineItem inline item={entry} key={entry.id}>
            <span data-incomplete={entry.incomplete ? 'true' : undefined}>{entry.summary}</span>
          </TimelineItem>
        ))}
      </div>,
    )
  }
  return rendered
}

function inferStatus(items: TimelineItemProjection[]) {
  const finalSummary = items.at(-1)?.summary.toLowerCase() ?? ''
  if (finalSummary.includes('cancel')) return 'cancelled'
  if (finalSummary.includes('supersed')) return 'superseded'
  if (finalSummary.includes('interrupt') || finalSummary.includes('force-stop'))
    return 'interrupted'
  if (finalSummary.includes('fail')) return 'failed'
  if (finalSummary.includes('complete')) return 'completed'
  return 'running'
}

function formatDuration(run: RunProjection) {
  const startedAt = Date.parse(run.startedAt)
  const endedAt = run.endedAt ? Date.parse(run.endedAt) : Date.now()
  if (!Number.isFinite(startedAt) || !Number.isFinite(endedAt)) return null
  const seconds = Math.max(0, Math.round((endedAt - startedAt) / 1_000))
  if (seconds < 60) return `${seconds}s`
  return `${Math.floor(seconds / 60)}m ${seconds % 60}s`
}
