import type { TaskEventEnvelope, TimelineItemProjection } from '@/generated/daemon-protocol'

import { ArtifactText } from './DiffPanel'
import { ProjectionList } from './EnvironmentPanel'

export function SourcesPanel({
  events,
  loading,
  missing,
  text,
  timeline,
}: {
  events: TaskEventEnvelope[]
  loading: boolean
  missing: boolean
  text: string | null
  timeline: TimelineItemProjection[]
}) {
  if (loading || missing || text !== null) {
    return (
      <ArtifactText
        empty="Select a source event to inspect it."
        loading={loading}
        missing={missing}
        text={text}
      />
    )
  }
  const projectedSources = timeline.filter((item) => item.kind === 'image')
  if (projectedSources.length > 0) return <ProjectionList items={projectedSources} />
  const sourceEvents = events.filter(
    (event) => event.source.kind === 'tool' || event.source.kind === 'assistant',
  )
  if (sourceEvents.length === 0) {
    return <p className="p-4 text-muted-foreground text-sm">No source artifacts recorded.</p>
  }
  return (
    <ul className="divide-y divide-border/70">
      {sourceEvents.map((event) => (
        <li className="px-4 py-3 text-sm" key={event.eventId}>
          {event.eventType}
        </li>
      ))}
    </ul>
  )
}
