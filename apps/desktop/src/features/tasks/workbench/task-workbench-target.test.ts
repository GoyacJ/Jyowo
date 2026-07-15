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
    for (const kind of [
      'command',
      'error',
      'notice',
      'permission',
      'tool_activity',
      'assistant_text',
    ] as const) {
      expect(taskWorkbenchTargetFromTimelineItem(item(kind), taskId)).toBeNull()
    }
    expect(
      taskWorkbenchTargetFromTimelineItem({ ...item('user_message'), blobId: undefined }, taskId),
    ).toBeNull()
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
