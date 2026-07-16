import { normalizeArtifactDescriptor } from '@/features/artifacts/model'
import type { TimelineItemProjection } from '@/generated/daemon-protocol'
import type { TaskWorkbenchTarget } from '@/shared/state/workbench-selection'

export function taskWorkbenchTargetFromTimelineItem(
  item: TimelineItemProjection,
  taskId: string,
  title = item.summary,
): TaskWorkbenchTarget | null {
  const artifactBlock = item.contentBlocks?.find((block) => block.type === 'artifact')
  const artifact = artifactBlock ? normalizeArtifactDescriptor(artifactBlock.artifact) : undefined
  if (artifact && (artifact.blobId || artifact.presentation?.previewBlobId || artifact.preview)) {
    const artifactKind = artifact.artifactKind ?? 'artifact'
    if (artifactKind === 'command' || artifactKind === 'terminal') return null
    const kind = artifactTargetKind(artifactKind)
    return {
      artifact: {
        artifactId: artifact.artifactId,
        artifactKind,
        format: artifact.format,
        mediaType: artifact.mediaType,
        preferredSurface: artifact.presentation?.preferredSurface,
        preview: artifact.preview,
        previewBlobId: artifact.presentation?.previewBlobId,
        size: artifact.size,
      },
      blobId: artifact.blobId ?? item.blobId ?? undefined,
      kind,
      resourceId:
        artifact.blobId ??
        artifact.presentation?.previewBlobId ??
        artifact.artifactId ??
        item.blobId ??
        item.semanticGroupId ??
        item.id,
      sourceEventId: item.id,
      taskId,
      title: artifact.title,
    }
  }
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
  if (
    item.kind === 'tool_activity' &&
    ['BrowserUse', 'BrowserDevTools'].includes(item.tool?.toolName ?? '')
  ) {
    return { ...shared, kind: 'browser', resourceId: taskId }
  }
  return null
}

function artifactTargetKind(artifactKind: string): TaskWorkbenchTarget['kind'] {
  if (artifactKind === 'diff' || artifactKind === 'patch') return 'diff'
  if (artifactKind === 'file') return 'file'
  if (artifactKind === 'image' || artifactKind === 'screenshot') return 'source'
  return 'artifact'
}

export function isTaskWorkbenchSidebarTarget(
  target: TaskWorkbenchTarget | null | undefined,
): target is TaskWorkbenchTarget {
  return Boolean(
    target && ['artifact', 'browser', 'diff', 'file', 'source', 'subagent'].includes(target.kind),
  )
}
