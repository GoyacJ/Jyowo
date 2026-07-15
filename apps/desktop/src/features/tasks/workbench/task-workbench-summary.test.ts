import { describe, expect, it } from 'vitest'

import type {
  TaskEventEnvelope,
  TaskProjection,
  TimelineItemProjection,
} from '@/generated/daemon-protocol'

import { taskWorkbenchSummaryItems } from './task-workbench-summary'

describe('taskWorkbenchSummaryItems', () => {
  it('derives only available task context with stable targets and statuses', () => {
    const items = taskWorkbenchSummaryItems({
      events,
      labels: { subagents: '子智能体' },
      projection,
      timeline,
    })

    expect(items.map((item) => item.id)).toEqual([
      'changes',
      'environment',
      'sources',
      'artifacts',
      'subagents',
    ])
    expect(items.find((item) => item.id === 'changes')).toMatchObject({
      count: 1,
      status: 'complete',
      target: { kind: 'diff', resourceId: 'diff-blob' },
    })
    expect(items.find((item) => item.id === 'subagents')).toMatchObject({
      count: 1,
      status: 'running',
    })
    expect(items.find((item) => item.id === 'artifacts')).toMatchObject({
      count: 2,
      target: { kind: 'artifact', resourceId: 'artifact-blob' },
    })
    expect(items.find((item) => item.id === 'environment')?.target).toBeUndefined()
  })

  it('does not create placeholder rows for absent context', () => {
    expect(
      taskWorkbenchSummaryItems({
        events: [],
        labels: { subagents: '子智能体' },
        projection: { ...projection, subagents: [], workspace: null },
        timeline: [],
      }),
    ).toEqual([])
  })

  it('uses the supplied localized subagent label when no summary is available', () => {
    const items = taskWorkbenchSummaryItems({
      events: [],
      labels: { subagents: '子智能体' },
      projection: {
        ...projection,
        subagents: projection.subagents?.map((subagent) => ({ ...subagent, summary: undefined })),
      },
      timeline: [],
    })

    expect(items.find((item) => item.id === 'subagents')?.target?.title).toBe('子智能体')
  })

  it('separates partial subagent failure counts from running counts', () => {
    const subagents = [
      ...(projection.subagents ?? []),
      {
        ...(projection.subagents?.[0] as NonNullable<TaskProjection['subagents']>[number]),
        childTaskId: 'child-failed',
        state: 'failed' as const,
      },
      {
        ...(projection.subagents?.[0] as NonNullable<TaskProjection['subagents']>[number]),
        childTaskId: 'child-complete',
        state: 'completed' as const,
      },
    ]
    const items = taskWorkbenchSummaryItems({
      events: [],
      labels: { subagents: '子智能体' },
      projection: { ...projection, subagents },
      timeline: [],
    })

    expect(items.find((item) => item.id === 'subagents')).toMatchObject({
      count: 3,
      failedCount: 1,
      runningCount: 1,
      status: 'failed',
    })
  })

  it('keeps errors out of the file and agent context rail', () => {
    const items = taskWorkbenchSummaryItems({
      events: [],
      labels: { subagents: 'Subagents' },
      projection: { ...projection, state: 'completed' },
      timeline: [item(1, 'error', 'Historical failure', 'old-error')],
    })

    expect(items.map((item) => item.id)).toEqual(['environment', 'subagents'])
    expect(items.some((item) => item.detail === 'Historical failure')).toBe(false)
  })
})

const taskId = '01J00000000000000000000001'

const projection: TaskProjection = {
  archived: false,
  lastGlobalOffset: 6,
  queue: [],
  state: 'running',
  streamVersion: 6,
  subagents: [
    {
      actorId: '01J00000000000000000000002',
      childTaskId: '01J00000000000000000000003',
      contextCursor: 2,
      delegationId: '01J00000000000000000000004',
      detached: false,
      parentSegmentId: '01J00000000000000000000005',
      parentTaskId: taskId,
      segmentId: '01J00000000000000000000006',
      startedAt: '2026-07-14T00:00:00Z',
      state: 'running',
      summary: 'Reviewing the workbench',
    },
  ],
  taskId,
  title: 'Workbench redesign',
  workspace: { mode: 'current', root: '/repo/Jyowo' },
}

const timeline: TimelineItemProjection[] = [
  item(1, 'diff', '3 files changed', 'diff-event', false, 'diff-blob'),
  item(2, 'command', 'pnpm test', 'command-event', true, 'command-blob'),
  item(3, 'image', 'Reference image', 'source-event', false, 'source-blob'),
  item(4, 'file', 'report.md', 'file-event', false, 'file-blob'),
  item(5, 'artifact', 'demo.mp4', 'artifact-event', false, 'artifact-blob'),
  item(6, 'error', 'Build failed', 'error-event'),
]

const events: TaskEventEnvelope[] = [
  {
    eventId: 'workspace-event',
    eventType: 'workspace.acquired',
    globalOffset: 1,
    payload: {},
    recordedAt: '2026-07-14T00:00:00Z',
    schemaVersion: 1,
    source: { kind: 'engine' },
    streamSequence: 1,
    taskId,
  },
]

function item(
  globalOffset: number,
  kind: TimelineItemProjection['kind'],
  summary: string,
  id: string,
  incomplete = false,
  blobId?: string,
): TimelineItemProjection {
  return { blobId, globalOffset, id, incomplete, kind, summary }
}
