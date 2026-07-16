import type {
  TaskEventEnvelope,
  TaskProjection,
  TimelineItemProjection,
} from '@/generated/daemon-protocol'
import type { TaskWorkbenchTarget } from '@/shared/state/workbench-selection'
import { taskWorkbenchTargetFromTimelineItem } from './task-workbench-target'

type TaskWorkbenchSummaryGroup = 'activity' | 'environment' | 'sources' | 'subagents'

export type TaskWorkbenchSummaryItem = {
  count?: number
  detail: string
  failedCount?: number
  group: TaskWorkbenchSummaryGroup
  id: 'artifacts' | 'audit' | 'changes' | 'commands' | 'environment' | 'sources' | 'subagents'
  runningCount?: number
  status: 'complete' | 'failed' | 'idle' | 'running'
  target?: TaskWorkbenchTarget
}

export function taskWorkbenchSummaryItems(input: {
  events: TaskEventEnvelope[]
  labels: { environment: string; subagents: string }
  projection: TaskProjection
  timeline: TimelineItemProjection[]
}): TaskWorkbenchSummaryItem[] {
  const { events, labels, projection, timeline } = input
  const items: TaskWorkbenchSummaryItem[] = []
  const changes = timeline.filter((item) => item.kind === 'diff')
  const commands = timeline.filter(
    (item) => item.kind === 'command' || item.tool?.operation === 'command',
  )
  const errors = timeline.filter((item) => item.kind === 'error')
  const sources = timeline.filter((item) => item.kind === 'image')
  const artifacts = timeline.filter((item) => item.kind === 'artifact' || item.kind === 'file')

  const latestChange = changes.at(-1)
  const changeTarget = latestChange
    ? taskWorkbenchTargetFromTimelineItem(latestChange, projection.taskId)
    : null
  if (latestChange && changeTarget) {
    items.push({
      count: changes.length,
      detail: latestChange.summary,
      group: 'environment',
      id: 'changes',
      status: latestChange.incomplete ? 'running' : 'complete',
      target: changeTarget,
    })
  }

  const workspaceEvent = [...events]
    .reverse()
    .find((event) => event.eventType.startsWith('workspace.'))
  if (projection.workspace || workspaceEvent) {
    const root = projection.workspace?.root ?? workspaceEvent?.eventType ?? ''
    items.push({
      detail: root,
      group: 'environment',
      id: 'environment',
      status: 'idle',
      target: {
        kind: 'environment',
        resourceId: projection.workspace ? 'workspace' : (workspaceEvent?.eventId ?? 'all'),
        sourceEventId: workspaceEvent?.eventId,
        taskId: projection.taskId,
        title: labels.environment,
      },
    })
  }

  const latestCommand = commands.at(-1)
  const commandTarget = latestCommand
    ? taskWorkbenchTargetFromTimelineItem(latestCommand, projection.taskId)
    : null
  if (latestCommand && commandTarget) {
    const commandStatus = latestCommand.tool?.status
    items.push({
      count: commands.length,
      detail: latestCommand.summary,
      group: 'activity',
      id: 'commands',
      status:
        commandStatus === 'failed' || commandStatus === 'denied'
          ? 'failed'
          : latestCommand.incomplete || commandStatus === 'requested' || commandStatus === 'running'
            ? 'running'
            : 'complete',
      target: commandTarget,
    })
  }

  const latestError = errors.at(-1)
  const auditTarget = latestError
    ? taskWorkbenchTargetFromTimelineItem(latestError, projection.taskId)
    : null
  if (latestError && auditTarget) {
    items.push({
      count: errors.length,
      detail: latestError.summary,
      group: 'activity',
      id: 'audit',
      status: 'failed',
      target: auditTarget,
    })
  }

  const latestSource = sources.at(-1)
  const sourceTarget = latestSource
    ? taskWorkbenchTargetFromTimelineItem(latestSource, projection.taskId)
    : null
  if (latestSource && sourceTarget) {
    items.push({
      count: sources.length,
      detail: latestSource.summary,
      group: 'sources',
      id: 'sources',
      status: latestSource.incomplete ? 'running' : 'complete',
      target: sourceTarget,
    })
  }

  const latestArtifact = artifacts.at(-1)
  const artifactTarget = latestArtifact
    ? taskWorkbenchTargetFromTimelineItem(latestArtifact, projection.taskId)
    : null
  if (latestArtifact && artifactTarget) {
    items.push({
      count: artifacts.length,
      detail: latestArtifact.summary,
      group: 'sources',
      id: 'artifacts',
      status: latestArtifact.incomplete ? 'running' : 'complete',
      target: artifactTarget,
    })
  }

  const subagents = projection.subagents ?? []
  if (subagents.length > 0) {
    const running = subagents.filter((agent) =>
      ['background', 'running', 'starting', 'yielding'].includes(agent.state),
    ).length
    const failed = subagents.filter((agent) => agent.state === 'failed').length
    const latest = subagents.at(-1)
    items.push({
      count: subagents.length,
      detail: latest?.summary ?? latest?.childTaskId ?? '',
      failedCount: failed,
      group: 'subagents',
      id: 'subagents',
      runningCount: running,
      status: failed > 0 ? 'failed' : running > 0 ? 'running' : 'complete',
      target: {
        kind: 'subagent',
        resourceId: 'all',
        taskId: projection.taskId,
        title: latest?.summary ?? labels.subagents,
      },
    })
  }

  return items
}
