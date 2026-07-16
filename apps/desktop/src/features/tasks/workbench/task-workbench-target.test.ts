import { describe, expect, it } from 'vitest'

import type { TimelineItemProjection } from '@/generated/daemon-protocol'
import { taskWorkbenchTargetFromTimelineItem } from './task-workbench-target'

describe('taskWorkbenchTargetFromTimelineItem', () => {
  it.each([
    ['diff', 'diff'],
    ['user_message', 'file'],
    ['file', 'file'],
    ['artifact', 'artifact'],
    ['subagent', 'subagent'],
    ['image', 'source'],
  ] as const)('maps %s timeline items to %s targets', (itemKind, targetKind) => {
    expect(taskWorkbenchTargetFromTimelineItem(item(itemKind), taskId)).toMatchObject({
      kind: targetKind,
      resourceId: blobId,
      sourceEventId: eventId,
      taskId,
    })
  })

  it('ignores commands, audit events, workspace notices, and narrative', () => {
    for (const kind of ['command', 'error', 'notice', 'permission', 'assistant_text'] as const) {
      expect(taskWorkbenchTargetFromTimelineItem(item(kind), taskId)).toBeNull()
    }
    expect(
      taskWorkbenchTargetFromTimelineItem({ ...item('user_message'), blobId: undefined }, taskId),
    ).toBeNull()
  })

  it.each(['BrowserUse', 'BrowserDevTools'])('maps %s activity to one task browser', (toolName) => {
    expect(
      taskWorkbenchTargetFromTimelineItem(
        {
          ...item('tool_activity'),
          tool: {
            operation: 'browse',
            status: 'completed',
            toolName,
            toolUseId: 'browser-tool',
          },
        },
        taskId,
      ),
    ).toMatchObject({ kind: 'browser', resourceId: taskId, taskId })
  })

  it('ignores non-browser tool activity', () => {
    expect(
      taskWorkbenchTargetFromTimelineItem(
        {
          ...item('tool_activity'),
          tool: {
            operation: 'read',
            status: 'completed',
            toolName: 'Read',
            toolUseId: 'read-tool',
          },
        },
        taskId,
      ),
    ).toBeNull()
  })

  it('opens an artifact block from an assistant message without a top-level blob', () => {
    expect(
      taskWorkbenchTargetFromTimelineItem(
        {
          ...item('assistant_text'),
          blobId: undefined,
          contentBlocks: [
            {
              artifact: {
                artifactId: 'assistant-video',
                artifactKind: 'video',
                blobId,
                mediaType: 'video/mp4',
                title: 'demo.mp4',
              },
              type: 'artifact',
            },
          ],
        },
        taskId,
      ),
    ).toMatchObject({
      blobId,
      kind: 'artifact',
      resourceId: blobId,
      title: 'demo.mp4',
    })
  })

  it('opens preview-only artifacts and preserves preview metadata', () => {
    expect(
      taskWorkbenchTargetFromTimelineItem(
        {
          ...item('notice'),
          blobId: undefined,
          contentBlocks: [
            {
              artifact: {
                artifactId: 'preview-map',
                artifactKind: 'map',
                mediaType: 'application/geo+json',
                presentation: { previewBlobId: 'preview-blob' },
                title: 'area.geojson',
              },
              type: 'artifact',
            },
          ],
        },
        taskId,
      ),
    ).toMatchObject({
      artifact: { artifactId: 'preview-map', previewBlobId: 'preview-blob' },
      blobId: undefined,
      kind: 'artifact',
      resourceId: 'preview-blob',
      title: 'area.geojson',
    })
  })

  it('uses the derived semantic group when an artifact has no persistent resource id', () => {
    expect(
      taskWorkbenchTargetFromTimelineItem(
        {
          ...item('assistant_text'),
          blobId: undefined,
          contentBlocks: [
            {
              artifact: {
                artifactKind: 'map',
                mediaType: 'application/geo+json',
                preview: '{"type":"FeatureCollection","features":[]}',
                title: 'inline map',
              },
              type: 'artifact',
            },
          ],
          semanticGroupId: 'message:artifact:1',
        },
        taskId,
      ),
    ).toMatchObject({ resourceId: 'message:artifact:1' })
  })
})

function item(
  kind: TimelineItemProjection['kind'],
  summary: string = kind,
): TimelineItemProjection {
  return {
    blobId,
    globalOffset: 1,
    id: eventId,
    incomplete: false,
    kind,
    summary,
  }
}

const taskId = '01J00000000000000000000001'
const eventId = '01J00000000000000000000002'
const blobId = '01J00000000000000000000003'
