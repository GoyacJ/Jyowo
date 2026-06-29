import { assertNever } from '@/shared/events/assert-never'
import type { RunEvent } from '@/shared/events/run-event-schema'

import type { ActivityRailItem } from './ActivityItem'
import type { PermissionRequestDetails } from './PermissionDialog'
import type { RawJsonDetails } from './RawJsonView'

export type RunEventViewModel = {
  activityItem: ActivityRailItem
  details?: {
    permissions?: PermissionRequestDetails[]
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
  const pendingPermissions = new Map<string, PermissionRequestDetails>()

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
    case 'permission.requested':
    case 'permission.resolved':
      return event.payload?.requestId ?? 'permission'
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
      return 'tool'
    case 'permission.requested':
    case 'permission.resolved':
      return 'permission'
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
    case 'permission.resolved':
    case 'run.ended':
    case 'tool.completed':
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
            state: 'pending',
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
    case 'artifact.created':
    case 'artifact.updated':
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
