import { describe, expect, it } from 'vitest'
import type { AssistantSegment, AssistantWork, ProcessStep } from '@/shared/tauri/commands'
import {
  buildTimelineRenderBlocks,
  getDefaultRenderBlockDisclosure,
  type TimelineRenderBlock,
} from './timeline-render-blocks'

describe('buildTimelineRenderBlocks', () => {
  it('keeps text segments as assistant text blocks sorted by projected order', () => {
    const assistant = assistantWork([
      textSegment('text-later', 20, 'later'),
      textSegment('text-first', 10, 'first'),
    ])

    const blocks = buildTimelineRenderBlocks(assistant)

    expect(blocks.map((block) => [block.kind, block.id, block.order])).toEqual([
      ['assistantText', 'text-first', 10],
      ['assistantText', 'text-later', 20],
    ])
    expect(blocks[0]).toMatchObject({
      kind: 'assistantText',
      segment: expect.objectContaining({ body: 'first' }),
    })
  })

  it('groups adjacent file edit and diff steps into one stable file edit block', () => {
    const assistant = assistantWork([
      processSegment('process-1', 10, [
        fileEditStep('edit-1', 20),
        diffStep('diff-1', 30, 'changeset-1', 'src/app.ts', 7, 2),
      ]),
    ])

    const blocks = buildTimelineRenderBlocks(assistant)

    expect(blocks).toHaveLength(1)
    expect(blocks[0]).toMatchObject({
      kind: 'fileEdit',
      id: 'process:process-1:file-edit:edit-1',
      processSegmentId: 'process-1',
      defaultOpen: false,
      forcedOpen: false,
    })
    expect(blocks[0].kind === 'fileEdit' ? blocks[0].steps.map((step) => step.id) : []).toEqual([
      'edit-1',
      'diff-1',
    ])
    expect(blocks[0].kind === 'fileEdit' ? blocks[0].files : []).toEqual([
      {
        changeSetId: 'changeset-1',
        path: 'src/app.ts',
        status: 'modified',
        addedLines: 7,
        removedLines: 2,
        preview: 'preview',
        fullPatchRef: 'patch-1',
        riskFlags: ['generated'],
      },
    ])
  })

  it('groups adjacent read and search activity steps into one collapsed activity block with items', () => {
    const assistant = assistantWork([
      processSegment('process-activity', 10, [
        activityStep('read-1', 20, 'fileRead', 'Read files', 2, [
          { kind: 'file', label: 'src/main.ts' },
        ]),
        activityStep('search-1', 30, 'fileSearch', 'Searched code', 1, [
          { kind: 'search', label: 'TimelineRenderBlock', detail: 'src' },
        ]),
      ]),
    ])

    const blocks = buildTimelineRenderBlocks(assistant)

    expect(blocks).toHaveLength(1)
    expect(blocks[0]).toMatchObject({
      kind: 'activity',
      id: 'process:process-activity:activity:read-1',
      title: 'Read files',
      itemCount: 3,
      defaultOpen: false,
      forcedOpen: false,
    })
    expect(blocks[0].kind === 'activity' ? blocks[0].items : []).toEqual([
      { id: 'read-1:file:src%2Fmain.ts:', kind: 'file', label: 'src/main.ts' },
      {
        id: 'search-1:search:TimelineRenderBlock:src',
        kind: 'search',
        label: 'TimelineRenderBlock',
        detail: 'src',
      },
    ])
  })

  it('groups adjacent command steps and forces failed or non-zero commands open', () => {
    const assistant = assistantWork([
      processSegment('process-command', 10, [
        commandStep('command-ok', 20, 'complete', 'git status --short', 0),
        commandStep('command-fail', 30, 'complete', 'cargo test', 101),
      ]),
    ])

    const blocks = buildTimelineRenderBlocks(assistant)

    expect(blocks).toHaveLength(1)
    expect(blocks[0]).toMatchObject({
      kind: 'commandGroup',
      id: 'process:process-command:commands:command-ok',
      defaultOpen: true,
      forcedOpen: true,
    })
    expect(blocks[0].kind === 'commandGroup' ? blocks[0].commands : []).toMatchObject([
      { id: 'command-ok', stepId: 'command-ok', status: 'complete' },
      { id: 'command-fail', stepId: 'command-fail', status: 'complete' },
    ])
  })

  it('preserves artifacts, review, clarification, notice, error, and agent activity in order', () => {
    const assistant = assistantWork([
      errorSegment('error-1', 60),
      artifactSegment('artifact-segment-1', 20),
      noticeSegment('notice-1', 50),
      reviewRequestSegment('review-1', 30),
      clarificationRequestSegment('clarification-1', 40),
      agentActivitySegment('agent-activity-1', 70),
    ])

    expect(buildTimelineRenderBlocks(assistant).map((block) => [block.kind, block.id])).toEqual([
      ['artifact', 'artifact-segment-1'],
      ['reviewRequest', 'review-1'],
      ['clarificationRequest', 'clarification-1'],
      ['notice', 'notice-1'],
      ['error', 'error-1'],
      ['agentActivity', 'agent-activity-1'],
    ])
  })

  it('suppresses duplicate image artifacts even when the artifact segment is ordered before process', () => {
    const blocks = buildTimelineRenderBlocks(
      assistantWork([
        artifactSegment('artifact-before-process', 1, {
          artifactId: 'artifact-image',
          revisionId: 'revision-latest',
          mediaKind: 'image',
        }),
        processSegment('process-image', 10, [
          artifactStep('artifact-step', 20, {
            artifactId: 'artifact-image',
            revisionId: 'revision-process',
          }),
        ]),
      ]),
    )

    expect(blocks.filter((block) => block.kind === 'artifact')).toHaveLength(1)
    expect(blocks[0]).toMatchObject({
      kind: 'artifact',
      id: 'process:process-image:artifact:artifact-step',
      segment: {
        artifactId: 'artifact-image',
        revision: {
          revisionId: 'revision-process',
          media: { kind: 'image' },
        },
      },
    })
  })

  it('creates process image artifact blocks when the artifact segment is not projected yet', () => {
    const blocks = buildTimelineRenderBlocks(
      assistantWork([
        processSegment('process-image', 10, [
          artifactStep('artifact-step', 20, {
            artifactId: 'artifact-image',
            revisionId: 'revision-process',
          }),
        ]),
      ]),
    )

    expect(blocks).toHaveLength(1)
    expect(blocks[0]).toMatchObject({
      kind: 'artifact',
      id: 'process:process-image:artifact:artifact-step',
      segment: {
        artifactId: 'artifact-image',
        title: 'Generated image',
        revision: {
          artifactId: 'artifact-image',
          revisionId: 'revision-process',
          kind: 'image',
          media: { kind: 'image' },
        },
      },
    })
  })

  it('does not create raw event blocks for unknown data', () => {
    const assistant = assistantWork([
      textSegment('text-1', 10, 'safe text'),
      {
        kind: 'rawEvent',
        id: 'raw-1',
        order: 20,
        payload: { secret: 'hidden' },
      } as unknown as AssistantSegment,
    ])

    const blocks = buildTimelineRenderBlocks(assistant)

    expect(blocks).toHaveLength(1)
    expect(blocks[0]?.kind).toBe('assistantText')
    expect(blocks.some((block) => block.kind === ('rawEvent' as TimelineRenderBlock['kind']))).toBe(
      false,
    )
  })

  it('keeps group ids stable when adjacent work is appended', () => {
    const before = buildTimelineRenderBlocks(
      assistantWork([
        processSegment('process-stable', 10, [
          fileEditStep('edit-1', 10),
          activityStep('read-1', 20, 'fileRead', 'Read files', 1),
          commandStep('command-1', 30, 'complete', 'pnpm test', 0),
        ]),
      ]),
    )
    const after = buildTimelineRenderBlocks(
      assistantWork([
        processSegment('process-stable', 10, [
          fileEditStep('edit-1', 10),
          diffStep('diff-1', 11, 'changeset-1', 'src/app.ts', 1, 0),
          activityStep('read-1', 20, 'fileRead', 'Read files', 1),
          activityStep('search-1', 21, 'fileSearch', 'Searched code', 3),
          commandStep('command-1', 30, 'complete', 'pnpm test', 0),
          commandStep('command-2', 31, 'complete', 'pnpm lint', 0),
        ]),
      ]),
    )

    expect(idsByKind(before)).toEqual({
      activity: 'process:process-stable:activity:read-1',
      commandGroup: 'process:process-stable:commands:command-1',
      fileEdit: 'process:process-stable:file-edit:edit-1',
    })
    expect(idsByKind(after)).toEqual(idsByKind(before))
  })

  it('uses distinct ids for separate later groups in the same process segment', () => {
    const blocks = buildTimelineRenderBlocks(
      assistantWork([
        processSegment('process-separate', 10, [
          commandStep('command-1', 10, 'complete', 'pnpm test', 0),
          activityStep('read-1', 20, 'fileRead', 'Read files', 1),
          commandStep('command-2', 30, 'complete', 'pnpm lint', 0),
        ]),
      ]),
    )

    expect(
      blocks.filter((block) => block.kind === 'commandGroup').map((block) => block.id),
    ).toEqual([
      'process:process-separate:commands:command-1',
      'process:process-separate:commands:command-2',
    ])
  })
})

describe('getDefaultRenderBlockDisclosure', () => {
  it('keeps failed process-backed groups forced open', () => {
    const [activity] = buildTimelineRenderBlocks(
      assistantWork([
        processSegment('process-failed', 10, [
          activityStep('read-1', 10, 'fileRead', 'Read files', 1, [], 'failed'),
        ]),
      ]),
    )

    expect(getDefaultRenderBlockDisclosure(activity)).toEqual({
      defaultOpen: true,
      forcedOpen: true,
    })
  })
})

function assistantWork(segments: AssistantSegment[]): AssistantWork {
  return {
    id: 'assistant-1',
    runId: 'run-1',
    projectionVersion: 1,
    status: 'complete',
    segments,
  }
}

function textSegment(id: string, order: number, body: string): AssistantSegment {
  return { kind: 'text', id, order, messageId: `${id}-message`, body }
}

function processSegment(id: string, order: number, steps: ProcessStep[]): AssistantSegment {
  return {
    kind: 'process',
    id,
    order,
    status: steps.some((step) => step.status === 'failed') ? 'failed' : 'complete',
    summary: 'process',
    steps,
  }
}

function fileEditStep(id: string, order: number): ProcessStep {
  return {
    id,
    order,
    kind: 'fileEdit',
    status: 'complete',
    title: 'Edited files',
    detail: { type: 'activity', summary: 'Edited files', itemCount: 1, items: [] },
  }
}

function diffStep(
  id: string,
  order: number,
  changeSetId: string,
  path: string,
  addedLines: number,
  removedLines: number,
): ProcessStep {
  return {
    id,
    order,
    kind: 'diff',
    status: 'complete',
    title: 'Diff ready',
    detail: {
      type: 'diff',
      id: changeSetId,
      summary: 'Edited 1 file',
      files: [
        {
          path,
          status: 'modified',
          addedLines,
          removedLines,
          preview: 'preview',
          fullPatchRef: 'patch-1',
          riskFlags: ['generated'],
        },
      ],
    },
  }
}

function activityStep(
  id: string,
  order: number,
  kind: 'fileRead' | 'fileSearch',
  summary: string,
  itemCount: number,
  items: NonNullable<Extract<ProcessStep['detail'], { type: 'activity' }>['items']> = [],
  status: ProcessStep['status'] = 'complete',
): ProcessStep {
  return {
    id,
    order,
    kind,
    status,
    title: summary,
    detail: { type: 'activity', summary, itemCount, items },
  }
}

function commandStep(
  id: string,
  order: number,
  status: ProcessStep['status'],
  command: string,
  exitCode?: number,
): ProcessStep {
  return {
    id,
    order,
    kind: 'command',
    status,
    title: command,
    detail: {
      type: 'command',
      command,
      exitCode,
      stdoutPreview: 'stdout',
      truncated: false,
      redactionState: 'redacted',
      riskLevel: 'low',
    },
  }
}

function artifactStep(
  id: string,
  order: number,
  {
    artifactId,
    revisionId,
  }: {
    artifactId: string
    revisionId?: string
  },
): ProcessStep {
  return {
    id,
    order,
    kind: 'artifact',
    status: 'complete',
    title: 'Generated image',
    detail: {
      type: 'artifact',
      artifactId,
      revisionId,
      media: {
        kind: 'image',
        mimeType: 'image/png',
        sizeBytes: 68,
      },
    },
  }
}

function artifactSegment(
  id: string,
  order: number,
  {
    artifactId = 'artifact-1',
    mediaKind,
    revisionId = 'revision-1',
  }: {
    artifactId?: string
    mediaKind?: 'image'
    revisionId?: string
  } = {},
): AssistantSegment {
  return {
    kind: 'artifact',
    id,
    order,
    artifactId,
    status: 'ready',
    title: 'Artifact',
    revision: {
      artifactId,
      revisionId,
      kind: mediaKind ?? 'code',
      status: 'ready',
      sourceRunId: 'run-1',
      title: 'Artifact',
      ...(mediaKind === 'image'
        ? {
            media: {
              kind: 'image',
              mimeType: 'image/png',
              sizeBytes: 68,
            },
          }
        : {}),
    },
  }
}

function reviewRequestSegment(id: string, order: number): AssistantSegment {
  return { kind: 'reviewRequest', id, order, requestId: 'review-request-1', title: 'Review' }
}

function clarificationRequestSegment(id: string, order: number): AssistantSegment {
  return {
    kind: 'clarificationRequest',
    id,
    order,
    requestId: 'clarification-request-1',
    prompt: 'Clarify',
  }
}

function noticeSegment(id: string, order: number): AssistantSegment {
  return { kind: 'notice', id, order, body: 'Notice' }
}

function errorSegment(id: string, order: number): AssistantSegment {
  return { kind: 'error', id, order, body: 'Error' }
}

function agentActivitySegment(id: string, order: number): AssistantSegment {
  return {
    kind: 'agentActivity',
    id,
    order,
    activityKind: 'subagent',
    agentId: 'agent-1',
    role: 'reviewer',
    taskSummary: 'Review code',
    status: 'completed',
  }
}

function idsByKind(blocks: TimelineRenderBlock[]) {
  return Object.fromEntries(
    blocks
      .filter(
        (block) =>
          block.kind === 'fileEdit' || block.kind === 'activity' || block.kind === 'commandGroup',
      )
      .map((block) => [block.kind, block.id]),
  )
}
