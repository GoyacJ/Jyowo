import type {
  TimelineArtifactProjection,
  TimelineArtifactSurface,
  TimelineContentBlock,
  TimelineItemProjection,
  TimelineNoticeLevel,
  TimelineTextFormat,
  TimelineToolProjection,
} from '@/generated/daemon-protocol'

export type ArtifactSurface = TimelineArtifactSurface

export type ArtifactDescriptor = {
  artifactId?: string
  artifactKind?: string
  blobId?: string
  format?: string
  mediaType: string
  presentation?: {
    preferredSurface?: ArtifactSurface
    previewBlobId?: string
  }
  preview?: string
  size?: number
  title: string
}

export type ContentBlock =
  | { format: TimelineTextFormat; text: string; type: 'text' }
  | { artifact: ArtifactDescriptor; type: 'artifact' }
  | { activity: TimelineToolProjection; type: 'tool_activity' }
  | { level: TimelineNoticeLevel; text: string; type: 'notice' }

export function timelineContentBlocks(item: TimelineItemProjection): ContentBlock[] {
  if (item.contentBlocks && item.contentBlocks.length > 0) {
    return item.contentBlocks.map(normalizeContentBlock)
  }
  if (item.kind === 'assistant_text' || item.kind === 'user_message') {
    const blocks: ContentBlock[] = [
      {
        format: item.kind === 'assistant_text' ? 'markdown' : 'plain',
        text: item.summary,
        type: 'text',
      },
    ]
    if (item.kind === 'user_message' && item.blobId) {
      blocks.push({ artifact: legacyUserAttachmentDescriptor(item), type: 'artifact' })
    }
    return blocks
  }
  if (item.kind === 'tool_activity' && item.tool) {
    return [{ activity: item.tool, type: 'tool_activity' }]
  }
  const artifact = legacyArtifactDescriptor(item)
  if (artifact) return [{ artifact, type: 'artifact' }]
  return [
    {
      level: item.kind === 'error' ? 'error' : item.kind === 'permission' ? 'warning' : 'info',
      text: item.summary,
      type: 'notice',
    },
  ]
}

function legacyUserAttachmentDescriptor(item: TimelineItemProjection): ArtifactDescriptor {
  return {
    artifactKind: 'file',
    blobId: item.blobId ?? undefined,
    format: inferFormat(item.summary),
    mediaType: 'application/octet-stream',
    presentation: { preferredSurface: 'card' },
    title: item.summary,
  }
}

export function normalizeTimelineItem(item: TimelineItemProjection): TimelineItemProjection {
  const contentBlocks = timelineContentBlocks(item)
  const activity = contentBlocks.find(
    (block): block is Extract<ContentBlock, { type: 'tool_activity' }> =>
      block.type === 'tool_activity',
  )?.activity
  const artifact = contentBlocks.find(
    (block): block is Extract<ContentBlock, { type: 'artifact' }> => block.type === 'artifact',
  )?.artifact
  return {
    ...item,
    blobId: item.blobId ?? artifact?.blobId,
    contentBlocks,
    tool: activity ?? item.tool,
  }
}

export function artifactDescriptorFromTimelineItem(
  item: TimelineItemProjection,
): ArtifactDescriptor | null {
  const block = timelineContentBlocks(item).find(
    (candidate): candidate is Extract<ContentBlock, { type: 'artifact' }> =>
      candidate.type === 'artifact',
  )
  return block?.artifact ?? null
}

export function normalizeArtifactDescriptor(
  artifact: TimelineArtifactProjection | ArtifactDescriptor,
): ArtifactDescriptor {
  return {
    artifactId: artifact.artifactId ?? undefined,
    artifactKind: artifact.artifactKind?.toLowerCase() ?? undefined,
    blobId: artifact.blobId ?? undefined,
    format: artifact.format?.toLowerCase() ?? inferFormat(artifact.title),
    mediaType: normalizeMediaType(artifact.mediaType),
    presentation: artifact.presentation
      ? {
          preferredSurface: artifact.presentation.preferredSurface ?? undefined,
          previewBlobId: artifact.presentation.previewBlobId ?? undefined,
        }
      : undefined,
    preview: artifact.preview ?? undefined,
    size: artifact.size ?? undefined,
    title: artifact.title,
  }
}

export function normalizeMediaType(mediaType: string | null | undefined) {
  return mediaType?.toLowerCase().split(';', 1)[0]?.trim() || 'application/octet-stream'
}

export function isTextMediaType(mediaType: string) {
  const normalized = normalizeMediaType(mediaType)
  return (
    normalized.startsWith('text/') ||
    normalized.endsWith('+json') ||
    normalized.endsWith('+xml') ||
    [
      'application/javascript',
      'application/json',
      'application/toml',
      'application/xml',
      'application/x-httpd-php',
      'application/x-sh',
      'application/x-yaml',
    ].includes(normalized)
  )
}

export function isGeoJsonArtifact(artifact: ArtifactDescriptor) {
  const mediaType = normalizeMediaType(artifact.mediaType)
  return (
    artifact.artifactKind === 'map' ||
    artifact.artifactKind === 'geojson' ||
    artifact.format === 'geojson' ||
    mediaType === 'application/geo+json' ||
    mediaType === 'application/geojson' ||
    mediaType === 'application/vnd.geo+json'
  )
}

function normalizeContentBlock(block: TimelineContentBlock): ContentBlock {
  return block.type === 'artifact'
    ? { artifact: normalizeArtifactDescriptor(block.artifact), type: 'artifact' }
    : block
}

function legacyArtifactDescriptor(item: TimelineItemProjection): ArtifactDescriptor | null {
  if (!['artifact', 'command', 'diff', 'file', 'image'].includes(item.kind)) {
    return null
  }
  return {
    artifactKind: item.kind,
    blobId: item.blobId ?? undefined,
    format: inferFormat(item.summary),
    mediaType: 'application/octet-stream',
    presentation: {
      preferredSurface: item.kind === 'image' ? 'inline' : 'card',
    },
    title: item.summary,
  }
}

function inferFormat(title: string) {
  const filename = title.split(/[\\/]/).at(-1) ?? title
  const extension = filename.includes('.') ? filename.split('.').at(-1)?.toLowerCase() : undefined
  return extension && /^[a-z0-9][a-z0-9.+_-]{0,15}$/.test(extension) ? extension : undefined
}
