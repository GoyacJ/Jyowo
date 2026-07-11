import type { TaskEventEnvelope, TimelineItemProjection } from '@/generated/daemon-protocol'

import { EventList, ProjectionList } from './EnvironmentPanel'

export function AuditPanel({
  events,
  timeline,
}: {
  events: TaskEventEnvelope[]
  timeline: TimelineItemProjection[]
}) {
  if (events.length > 0) return <EventList events={events} />
  const projectedEvents = timeline.filter((item) =>
    ['compaction', 'error', 'notice', 'permission', 'tool_activity'].includes(item.kind),
  )
  if (projectedEvents.length > 0) return <ProjectionList items={projectedEvents} />
  return <p className="p-4 text-muted-foreground text-sm">No audit events recorded.</p>
}
