import { describe, expect, it } from 'vitest'

import type { TimelineItemProjection } from '@/generated/daemon-protocol'

import {
  artifactDescriptorFromTimelineItem,
  normalizeTimelineItem,
  timelineContentBlocks,
} from './model'

describe('timeline content adapter', () => {
  it('derives an artifact descriptor from a legacy timeline item', () => {
    const item = timelineItem({ blobId: 'blob-1', kind: 'image', summary: 'map.png' })
    expect(artifactDescriptorFromTimelineItem(item)).toMatchObject({
      artifactKind: 'image',
      blobId: 'blob-1',
      format: 'png',
      mediaType: 'application/octet-stream',
      title: 'map.png',
    })
  })

  it('normalizes protocol artifact metadata', () => {
    const item = timelineItem({
      contentBlocks: [
        {
          artifact: {
            artifactKind: 'MAP',
            blobId: 'blob-2',
            mediaType: 'Application/Geo+JSON; charset=utf-8',
            presentation: { preferredSurface: 'inline' },
            size: 42,
            title: 'area.geojson',
          },
          type: 'artifact',
        },
      ],
      kind: 'artifact',
    })
    expect(artifactDescriptorFromTimelineItem(item)).toEqual({
      artifactId: undefined,
      artifactKind: 'map',
      blobId: 'blob-2',
      format: 'geojson',
      mediaType: 'application/geo+json',
      presentation: { preferredSurface: 'inline', previewBlobId: undefined },
      preview: undefined,
      size: 42,
      title: 'area.geojson',
    })
  })

  it('treats protocol content blocks as the canonical message content', () => {
    const item = timelineItem({
      contentBlocks: [{ format: 'markdown', text: 'old', type: 'text' }],
      kind: 'assistant_text',
      summary: 'current',
    })
    expect(timelineContentBlocks(item)).toEqual([{ format: 'markdown', text: 'old', type: 'text' }])
  })

  it('keeps legacy fields as fallback when protocol blocks are absent', () => {
    expect(
      timelineContentBlocks(timelineItem({ kind: 'assistant_text', summary: 'fallback' })),
    ).toEqual([{ format: 'markdown', text: 'fallback', type: 'text' }])
    expect(
      timelineContentBlocks(
        timelineItem({ blobId: 'attachment-blob', kind: 'user_message', summary: 'photo.png' }),
      ),
    ).toEqual([
      { format: 'plain', text: 'photo.png', type: 'text' },
      expect.objectContaining({
        artifact: expect.objectContaining({ blobId: 'attachment-blob', artifactKind: 'file' }),
        type: 'artifact',
      }),
    ])
  })

  it('recovers compatibility fields from canonical blocks', () => {
    const tool = {
      operation: 'read' as const,
      status: 'completed' as const,
      toolName: 'read_file',
      toolUseId: 'tool-1',
    }
    const normalized = normalizeTimelineItem(
      timelineItem({
        blobId: undefined,
        contentBlocks: [
          {
            artifact: {
              artifactKind: 'image',
              blobId: 'canonical-blob',
              mediaType: 'image/png',
              title: 'image.png',
            },
            type: 'artifact',
          },
          { activity: tool, type: 'tool_activity' },
        ],
        tool: undefined,
      }),
    )

    expect(normalized.blobId).toBe('canonical-blob')
    expect(normalized.tool).toEqual(tool)
    expect(normalized.contentBlocks).toHaveLength(2)
  })
})

function timelineItem(overrides: Partial<TimelineItemProjection> = {}): TimelineItemProjection {
  return {
    globalOffset: 1,
    id: 'event-1',
    incomplete: false,
    kind: 'artifact',
    summary: 'artifact',
    ...overrides,
  }
}
