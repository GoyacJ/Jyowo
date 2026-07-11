import { assertNever } from '@/shared/events/assert-never'
import type { RunEvent } from '@/shared/events/run-event-schema'

import type { ActivityRailItem } from './ActivityItem'
import type { ActivityPermissionDetails } from './PermissionDialog'
import type { RawJsonDetails } from './RawJsonView'

export type RunEventViewModel = {
  activityItem: ActivityRailItem
  details?: {
    permissions?: ActivityPermissionDetails[]
  }
  order: {
    runId: string
    sequence: number
    timestamp: string
  }
  rawJson?: RawJsonDetails
}

export function toRunEventViewModels(events: RunEvent[]): RunEventViewModel[] {
  const viewModels = events.map(toRunEventViewModel)
  const pendingPermissions = new Map<string, ActivityPermissionDetails>()

  viewModels.forEach((viewModel) => {
    const permission = viewModel.details?.permissions?.[0]

    if (!permission) {
      return
    }

    if (permission.state === 'pending') {
      pendingPermissions.set(permission.id, permission)
      return
    }

    const pendingPermission = pendingPermissions.get(permission.id)

    if (!pendingPermission) {
      return
    }

    pendingPermission.label = permission.label
    pendingPermission.state = permission.state
    viewModel.details = undefined
  })

  return viewModels
}

export function toRunEventViewModel(event: RunEvent): RunEventViewModel {
  return {
    activityItem: {
      id: event.id,
      label: getActivityLabel(event),
      status: getActivityStatus(event),
      time: event.timestamp,
    },
    order: {
      runId: event.runId,
      sequence: event.sequence,
      timestamp: event.timestamp,
    },
    details: getDetails(event),
    rawJson: getRawJson(event),
  }
}

function getActivityLabel(event: RunEvent): string {
  if (event.visibility === 'withheld') {
    return getWithheldActivityLabel(event)
  }

  switch (event.type) {
    case 'run.started':
    case 'run.ended':
      return 'run'
    case 'user.message.appended':
      return 'user'
    case 'assistant.delta':
    case 'assistant.thinking.delta':
    case 'assistant.completed':
    case 'assistant.review.requested':
    case 'assistant.clarification.requested':
      return 'assistant'
    case 'assistant.notice':
      return 'notice'
    case 'tool.requested':
      return event.payload?.toolName ?? 'tool'
    case 'tool.approved':
    case 'tool.denied':
    case 'tool.completed':
    case 'tool.failed':
      return event.payload?.toolUseId ?? 'tool'
    case 'tool.deferred_pool_changed':
      return 'tool pool'
    case 'permission.requested':
    case 'permission.resolved':
      return event.payload?.requestId ?? 'permission'
    case 'subagent.spawned':
      return event.payload?.role ?? event.payload?.subagentId ?? 'subagent'
    case 'subagent.announced':
    case 'subagent.terminated':
    case 'subagent.stalled':
      return event.payload?.subagentId ?? 'subagent'
    case 'subagent.permission.forwarded':
    case 'subagent.permission.resolved':
      return event.payload?.requestId ?? 'subagent permission'
    case 'team.created':
      return event.payload?.name ?? event.payload?.teamId ?? 'team'
    case 'team.member.joined':
    case 'team.member.left':
    case 'team.member.stalled':
      return event.payload?.agentId ?? 'team member'
    case 'team.task.updated':
      return event.payload?.title ?? event.payload?.taskId ?? 'team task'
    case 'agent.message.sent':
    case 'agent.message.routed':
      return event.payload?.messageId ?? 'agent message'
    case 'team.turn.completed':
      return event.payload?.turnId ?? 'team turn'
    case 'team.terminated':
      return event.payload?.teamId ?? 'team'
    case 'background.started':
      return event.payload?.title ?? event.payload?.backgroundAgentId ?? 'background'
    case 'background.state.changed':
      return event.payload?.backgroundAgentId ?? 'background'
    case 'background.input.requested':
    case 'background.input.submitted':
      return event.payload?.requestId ?? 'background input'
    case 'background.permission.requested':
    case 'background.permission.resolved':
      return event.payload?.requestId ?? 'background permission'
    case 'background.cancelled':
    case 'background.completed':
    case 'background.failed':
    case 'background.interrupted':
    case 'background.archived':
    case 'background.deleted':
      return event.payload?.backgroundAgentId ?? 'background'
    case 'artifact.created':
    case 'artifact.updated':
      return event.payload?.artifactId ?? 'artifact'
    case 'engine.failed':
      return 'engine'
    case 'plugin.loaded':
    case 'plugin.rejected':
    case 'plugin.failed':
      return event.payload?.pluginName ?? 'plugin'
    default:
      return assertNever(event)
  }
}

function getWithheldActivityLabel(event: RunEvent): string {
  switch (event.type) {
    case 'run.started':
    case 'run.ended':
      return 'run'
    case 'user.message.appended':
      return 'user'
    case 'assistant.delta':
    case 'assistant.thinking.delta':
    case 'assistant.completed':
    case 'assistant.review.requested':
    case 'assistant.clarification.requested':
      return 'assistant'
    case 'assistant.notice':
      return 'notice'
    case 'tool.requested':
    case 'tool.approved':
    case 'tool.denied':
    case 'tool.completed':
    case 'tool.failed':
    case 'tool.deferred_pool_changed':
      return 'tool'
    case 'permission.requested':
    case 'permission.resolved':
      return 'permission'
    case 'subagent.spawned':
    case 'subagent.announced':
    case 'subagent.terminated':
    case 'subagent.stalled':
      return 'subagent'
    case 'subagent.permission.forwarded':
    case 'subagent.permission.resolved':
      return 'subagent permission'
    case 'team.created':
    case 'team.member.joined':
    case 'team.member.left':
    case 'team.member.stalled':
    case 'team.task.updated':
    case 'agent.message.sent':
    case 'agent.message.routed':
    case 'team.turn.completed':
    case 'team.terminated':
      return 'team'
    case 'background.started':
    case 'background.state.changed':
    case 'background.input.requested':
    case 'background.input.submitted':
    case 'background.cancelled':
    case 'background.completed':
    case 'background.failed':
    case 'background.interrupted':
    case 'background.archived':
    case 'background.deleted':
      return 'background'
    case 'background.permission.requested':
    case 'background.permission.resolved':
      return 'background permission'
    case 'artifact.created':
    case 'artifact.updated':
      return 'artifact'
    case 'engine.failed':
      return 'engine'
    case 'plugin.loaded':
    case 'plugin.rejected':
    case 'plugin.failed':
      return 'plugin'
    default:
      return assertNever(event)
  }
}

function getActivityStatus(event: RunEvent): ActivityRailItem['status'] {
  switch (event.type) {
    case 'run.started':
    case 'user.message.appended':
    case 'assistant.delta':
    case 'assistant.thinking.delta':
    case 'tool.approved':
      return 'running'
    case 'tool.requested':
      return 'queued'
    case 'permission.requested':
      if (event.payload?.autoResolved) {
        return 'success'
      }
      return 'blocked'
    case 'subagent.spawned':
    case 'subagent.stalled':
    case 'team.created':
    case 'team.member.joined':
    case 'team.task.updated':
    case 'agent.message.sent':
    case 'agent.message.routed':
    case 'team.turn.completed':
      return 'running'
    case 'subagent.announced':
      if (
        event.payload?.status === 'failed' ||
        event.payload?.status === 'stalled' ||
        event.payload?.status === 'max_iterations_reached' ||
        event.payload?.status === 'maxIterationsReached' ||
        event.payload?.status === 'max_budget'
      ) {
        return 'failed'
      }
      return 'success'
    case 'subagent.terminated':
      return event.payload?.reason === 'failed' ||
        event.payload?.reason === 'bridge_broken' ||
        event.payload?.reason === 'bridgeBroken'
        ? 'failed'
        : 'success'
    case 'subagent.permission.forwarded':
      return 'blocked'
    case 'subagent.permission.resolved':
      return 'success'
    case 'team.member.left':
      return event.payload?.reason === 'error' || event.payload?.reason === 'stalled_removed'
        ? 'failed'
        : 'success'
    case 'team.member.stalled':
      return 'failed'
    case 'team.terminated':
      return event.payload?.reason === 'error' || event.payload?.reason === 'member_failed'
        ? 'failed'
        : 'success'
    case 'background.started':
      return 'running'
    case 'background.state.changed':
      if (event.payload?.to === 'failed' || event.payload?.to === 'interrupted') {
        return 'failed'
      }
      if (
        event.payload?.to === 'waiting_for_permission' ||
        event.payload?.to === 'waiting_for_input' ||
        event.payload?.to === 'paused'
      ) {
        return 'blocked'
      }
      if (
        event.payload?.to === 'succeeded' ||
        event.payload?.to === 'cancelled' ||
        event.payload?.to === 'archived'
      ) {
        return 'success'
      }
      return 'running'
    case 'background.input.requested':
      return 'blocked'
    case 'background.input.submitted':
      return 'success'
    case 'background.permission.requested':
      return 'blocked'
    case 'background.failed':
    case 'background.interrupted':
      return 'failed'
    case 'background.cancelled':
    case 'background.completed':
    case 'background.archived':
    case 'background.deleted':
      return 'success'
    case 'tool.denied':
    case 'assistant.review.requested':
    case 'assistant.clarification.requested':
      return 'blocked'
    case 'tool.failed':
    case 'engine.failed':
    case 'plugin.rejected':
    case 'plugin.failed':
      return 'failed'
    case 'plugin.loaded':
      return 'success'
    case 'artifact.created':
    case 'artifact.updated':
      if (event.payload?.status === 'failed') {
        return 'failed'
      }
      if (event.payload?.status === 'running') {
        return 'running'
      }
      return 'success'
    case 'assistant.completed':
    case 'assistant.notice':
    case 'background.permission.resolved':
    case 'permission.resolved':
    case 'run.ended':
    case 'tool.completed':
    case 'tool.deferred_pool_changed':
      return 'success'
    default:
      return assertNever(event)
  }
}

function getDetails(event: RunEvent): RunEventViewModel['details'] {
  switch (event.type) {
    case 'permission.requested':
      if (event.visibility === 'withheld') {
        return undefined
      }

      if (!event.payload) {
        return undefined
      }

      return {
        permissions: [
          {
            decisionScope: event.payload.decisionScope,
            diffSummary: event.payload.diffSummary,
            exposure: event.payload.exposure,
            id: event.payload.requestId,
            label: event.payload.operation,
            operation: event.payload.operation,
            reason: event.payload.reason,
            risk: event.payload.severity,
            state: event.payload.autoResolved ? 'approved' : 'pending',
            target: event.payload.target,
            toolUseId: event.payload.toolUseId,
            workspaceBoundary: event.payload.workspaceBoundary,
          },
        ],
      }
    case 'permission.resolved':
      if (event.visibility === 'withheld') {
        return undefined
      }

      return {
        permissions: [
          {
            id: event.payload?.requestId ?? event.id,
            label: 'permission',
            risk: 'medium',
            state: event.payload?.decision === 'approve' ? 'approved' : 'denied',
          },
        ],
      }
    case 'background.permission.requested':
      if (event.visibility === 'withheld') {
        return undefined
      }

      return {
        permissions: [
          {
            id: event.payload?.requestId ?? event.id,
            label: 'background permission',
            reason: event.payload?.reason,
            risk: 'medium',
            state: 'pending',
          },
        ],
      }
    case 'background.permission.resolved':
      if (event.visibility === 'withheld') {
        return undefined
      }

      return {
        permissions: [
          {
            id: event.payload?.requestId ?? event.id,
            label: 'background permission',
            risk: 'medium',
            state: event.payload?.decision === 'approve' ? 'approved' : 'denied',
          },
        ],
      }
    case 'run.started':
    case 'run.ended':
    case 'user.message.appended':
    case 'assistant.delta':
    case 'assistant.thinking.delta':
    case 'assistant.completed':
    case 'assistant.review.requested':
    case 'assistant.clarification.requested':
    case 'assistant.notice':
    case 'tool.requested':
    case 'tool.approved':
    case 'tool.denied':
    case 'tool.completed':
    case 'tool.failed':
    case 'tool.deferred_pool_changed':
    case 'artifact.created':
    case 'artifact.updated':
    case 'background.started':
    case 'background.state.changed':
    case 'background.input.requested':
    case 'background.input.submitted':
    case 'background.cancelled':
    case 'background.completed':
    case 'background.failed':
    case 'background.interrupted':
    case 'background.archived':
    case 'background.deleted':
    case 'subagent.spawned':
    case 'subagent.announced':
    case 'subagent.terminated':
    case 'subagent.stalled':
    case 'subagent.permission.forwarded':
    case 'subagent.permission.resolved':
    case 'team.created':
    case 'team.member.joined':
    case 'team.member.left':
    case 'team.member.stalled':
    case 'team.task.updated':
    case 'agent.message.sent':
    case 'agent.message.routed':
    case 'team.turn.completed':
    case 'team.terminated':
    case 'engine.failed':
    case 'plugin.loaded':
    case 'plugin.rejected':
    case 'plugin.failed':
      return undefined
    default:
      return assertNever(event)
  }
}

function getRawJson(event: RunEvent): RawJsonDetails | undefined {
  if (event.visibility === 'withheld') {
    return {
      payload: {},
      withheld: true,
    }
  }

  if (event.visibility !== 'redacted') {
    return undefined
  }

  return {
    payload: event.payload ?? {},
  }
}
