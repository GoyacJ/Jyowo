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
  if (item.kind === 'command') {
    return {
      ...shared,
      artifact: item.blobId
        ? undefined
        : {
            artifactKind: 'command',
            mediaType: 'text/plain',
            preview: item.summary,
          },
      kind: 'command',
    }
  }
  if (item.kind === 'user_message' && item.blobId) return { ...shared, kind: 'file' }
  if (item.kind === 'file' && item.blobId) return { ...shared, kind: 'file' }
  if (item.kind === 'artifact' && item.blobId) return { ...shared, kind: 'artifact' }
  if (item.kind === 'subagent') return { ...shared, kind: 'subagent' }
  if (item.kind === 'image' && item.blobId) return { ...shared, kind: 'source' }
  if (item.kind === 'error') {
    return { ...shared, blobId: undefined, kind: 'audit', resourceId: item.id }
  }
  if (
    item.kind === 'tool_activity' &&
    item.tool?.operation === 'command' &&
    (item.tool.command || item.tool.output)
  ) {
    const preview = [item.tool.command ? `$ ${item.tool.command}` : null, item.tool.output]
      .filter(Boolean)
      .join('\n')
    return {
      ...shared,
      artifact: {
        artifactKind: 'command',
        mediaType: 'text/plain',
        preview,
      },
      blobId: undefined,
      kind: 'command',
      resourceId: item.tool.toolUseId,
    }
  }
  if (
    item.kind === 'tool_activity' &&
    ['BrowserUse', 'BrowserDevTools'].includes(item.tool?.toolName ?? '')
  ) {
    return { ...shared, kind: 'browser', resourceId: taskId }
  }
  return null
}

function artifactTargetKind(artifactKind: string): TaskWorkbenchTarget['kind'] {
  if (artifactKind === 'command' || artifactKind === 'terminal') return 'command'
  if (artifactKind === 'diff' || artifactKind === 'patch') return 'diff'
  if (artifactKind === 'file') return 'file'
  if (artifactKind === 'image' || artifactKind === 'screenshot') return 'source'
  return 'artifact'
}

export function isTaskWorkbenchSidebarTarget(
  target: TaskWorkbenchTarget | null | undefined,
): target is TaskWorkbenchTarget {
  return Boolean(
    target &&
      [
        'artifact',
        'audit',
        'browser',
        'command',
        'diff',
        'environment',
        'file',
        'source',
        'subagent',
      ].includes(target.kind),
  )
}
