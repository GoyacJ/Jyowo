import type { TimelineItemProjection } from '@/generated/daemon-protocol'
import type { TaskWorkbenchTarget } from '@/shared/state/workbench-selection'

export function taskWorkbenchTargetFromTimelineItem(
  item: TimelineItemProjection,
  taskId: string,
  title = item.summary,
): TaskWorkbenchTarget | null {
  const shared = {
    blobId: item.blobId ?? undefined,
    resourceId: item.blobId ?? item.id,
    sourceEventId: item.id,
    taskId,
    title,
  }

  if (item.kind === 'diff' && item.blobId) return { ...shared, kind: 'diff' }
  if (item.kind === 'user_message' && item.blobId) return { ...shared, kind: 'file' }
  if (item.kind === 'file' && item.blobId) return { ...shared, kind: 'file' }
  if (item.kind === 'artifact' && item.blobId) return { ...shared, kind: 'artifact' }
  if (item.kind === 'subagent') return { ...shared, kind: 'subagent' }
  if (item.kind === 'image' && item.blobId) return { ...shared, kind: 'source' }
  return null
}

export function isTaskWorkbenchSidebarTarget(
  target: TaskWorkbenchTarget | null | undefined,
): target is TaskWorkbenchTarget {
  return Boolean(target && ['artifact', 'diff', 'file', 'source', 'subagent'].includes(target.kind))
}
