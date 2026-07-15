import type { TFunction } from 'i18next'

import type { TimelineItemProjection } from '@/generated/daemon-protocol'
import { toolActivitySummary } from './tool-activity-summary'

const summaryKeys = {
  'Artifact updated': 'artifactUpdated',
  'Context compacted': 'contextCompacted',
  'Permission expired after restart': 'permissionExpired',
  'Permission requested': 'permissionRequested',
  'Permission resolved': 'permissionResolved',
  'Run cancelled': 'runCancelled',
  'Run completed': 'runCompleted',
  'Run failed': 'runFailed',
  'Run force-stop requested': 'runForceStopRequested',
  'Run force-stop timed out': 'runForceStopTimedOut',
  'Run force-stopped': 'runForceStopped',
  'Run interrupted by restart': 'runInterruptedByRestart',
  'Run safe point reached': 'runSafePointReached',
  'Run started': 'runStarted',
  'Run superseded': 'runSuperseded',
  'Run yield requested': 'runYieldRequested',
  'Subagent continuing in background': 'subagentBackground',
  'Subagent finished': 'subagentFinished',
  'Subagent linked': 'subagentLinked',
  'Subagent started': 'subagentStarted',
  'Subagent state changed': 'subagentStateChanged',
  'Subagent summary updated': 'subagentSummaryUpdated',
  'Task actor failed': 'taskActorFailed',
  'Task archived': 'taskArchived',
  'Task created': 'taskCreated',
  'Task pinned': 'taskPinned',
  'Task removed': 'taskRemoved',
  'Task restored': 'taskRestored',
  'Task title changed': 'taskTitleChanged',
  'Task unpinned': 'taskUnpinned',
  'Tool completed': 'toolCompleted',
  'Tool denied': 'toolDenied',
  'Tool outcome is indeterminate after restart': 'toolIndeterminate',
  'Tool started': 'toolStarted',
  'Unexpected error': 'unexpectedError',
  'Workspace acquired': 'workspaceAcquired',
  'Workspace cleanup blocked': 'workspaceCleanupBlocked',
  'Workspace cleanup pending': 'workspaceCleanupPending',
  'Workspace lease waiting': 'workspaceLeaseWaiting',
  'Workspace preparing': 'workspacePreparing',
  'Workspace released': 'workspaceReleased',
  'Workspace write override applied': 'workspaceWriteOverrideApplied',
} as const

export function timelineSummary(item: TimelineItemProjection, t: TFunction<'tasks'>) {
  if (item.kind === 'assistant_text' || item.kind === 'user_message') return item.summary
  if (item.kind === 'tool_activity' && item.tool) return toolActivitySummary(item, t)
  const key = summaryKeys[item.summary as keyof typeof summaryKeys]
  if (key) return t(`timeline.summary.${key}`)
  if (item.kind === 'tool_activity' && item.summary.startsWith('Using ')) {
    return t('timeline.summary.usingTool', { tool: item.summary.slice('Using '.length) })
  }
  return item.summary
}
