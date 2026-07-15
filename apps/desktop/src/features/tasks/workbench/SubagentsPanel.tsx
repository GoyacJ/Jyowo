import { useTranslation } from 'react-i18next'
import type { SubagentProjection, TaskEventEnvelope } from '@/generated/daemon-protocol'
import type { TaskWorkbenchTarget } from '@/shared/state/workbench-selection'

export function SubagentsPanel({
  events,
  subagents,
  target,
}: {
  events: TaskEventEnvelope[]
  subagents: SubagentProjection[]
  target: TaskWorkbenchTarget
}) {
  const { t } = useTranslation('tasks')
  const visibleSubagents = selectSubagents(subagents, events, target)
  if (visibleSubagents.length === 0) return <Empty>{t('workbench.empty.agents')}</Empty>
  return (
    <ul className="divide-y divide-border/70">
      {visibleSubagents.map((agent) => (
        <li className="space-y-1 px-4 py-3" key={agent.delegationId}>
          <div className="flex items-center justify-between gap-3">
            <span className="truncate text-sm">{agent.summary ?? agent.childTaskId}</span>
            <span className="shrink-0 text-muted-foreground text-xs">
              {t(`workbench.agentState.${agent.state}`)}
            </span>
          </div>
          <p className="truncate font-mono text-[11px] text-muted-foreground">
            {agent.childTaskId}
          </p>
        </li>
      ))}
    </ul>
  )
}

function selectSubagents(
  subagents: SubagentProjection[],
  events: TaskEventEnvelope[],
  target: TaskWorkbenchTarget,
) {
  if (target.resourceId === 'all') return subagents
  const direct = subagents.filter((agent) => subagentIds(agent).includes(target.resourceId))
  if (direct.length > 0) return direct

  const event = events.find(
    (item) => item.eventId === target.sourceEventId || item.eventId === target.resourceId,
  )
  const payload = asRecord(event?.payload)
  const child = asRecord(payload?.child) ?? payload
  if (!child) return []
  const ids = ['actorId', 'childTaskId', 'delegationId', 'segmentId']
    .map((key) => child[key])
    .filter((value): value is string => typeof value === 'string')
  return subagents.filter((agent) => subagentIds(agent).some((id) => ids.includes(id)))
}

function subagentIds(agent: SubagentProjection) {
  return [agent.actorId, agent.childTaskId, agent.delegationId, agent.segmentId]
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null
}

function Empty({ children }: { children: React.ReactNode }) {
  return (
    <p className="flex min-h-48 items-center justify-center px-6 text-center text-muted-foreground text-sm">
      {children}
    </p>
  )
}
