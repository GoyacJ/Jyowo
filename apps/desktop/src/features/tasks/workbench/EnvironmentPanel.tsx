import { useTranslation } from 'react-i18next'
import type { TaskEventEnvelope, TimelineItemProjection } from '@/generated/daemon-protocol'
import { timelineSummary } from '../timeline/timeline-summary'

export function EnvironmentPanel({
  events,
  timeline,
}: {
  events: TaskEventEnvelope[]
  timeline: TimelineItemProjection[]
}) {
  const { t } = useTranslation('tasks')
  const environmentEvents = events.filter((event) => event.eventType.startsWith('workspace.'))
  if (environmentEvents.length > 0) return <EventList events={environmentEvents} />
  const projectedEvents = timeline.filter(
    (item) => item.kind === 'notice' && item.summary.toLowerCase().includes('workspace'),
  )
  if (projectedEvents.length > 0) return <ProjectionList items={projectedEvents} />
  return <p className="p-4 text-muted-foreground text-sm">{t('workbench.empty.environment')}</p>
}

export function ProjectionList({ items }: { items: TimelineItemProjection[] }) {
  const { t } = useTranslation('tasks')
  return (
    <ol className="divide-y divide-border/70">
      {items.map((item) => (
        <li className="grid grid-cols-[1fr_auto] gap-3 px-4 py-3" key={item.id}>
          <span className="text-sm">{timelineSummary(item, t)}</span>
          <span className="font-mono text-[11px] text-muted-foreground">#{item.globalOffset}</span>
        </li>
      ))}
    </ol>
  )
}

function EventList({ events }: { events: TaskEventEnvelope[] }) {
  return (
    <ol className="divide-y divide-border/70">
      {events.map((event) => (
        <li className="grid grid-cols-[1fr_auto] gap-3 px-4 py-3" key={event.eventId}>
          <span className="text-sm">{event.eventType}</span>
          <span className="font-mono text-[11px] text-muted-foreground">#{event.globalOffset}</span>
        </li>
      ))}
    </ol>
  )
}
