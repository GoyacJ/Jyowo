import { useTranslation } from 'react-i18next'
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
  const { t } = useTranslation('tasks')
  if (loading || missing || text !== null) {
    return (
      <ArtifactText
        empty={t('workbench.empty.source')}
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
    return <p className="p-4 text-muted-foreground text-sm">{t('workbench.empty.sources')}</p>
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
