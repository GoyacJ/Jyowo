import type { Meta, StoryObj } from '@storybook/react-vite'
import { useEffect, useState } from 'react'
import { TaskWorkspaceView } from '@/features/tasks/TaskWorkspace'
import type { TaskSnapshot } from '@/features/tasks/task-store'
import type {
  ClientRequest,
  QueueItemProjection,
  TaskProjection,
  TimelineItemProjection,
} from '@/generated/daemon-protocol'
import { appI18n } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'
import type { TaskWorkbenchTarget } from '@/shared/state/workbench-selection'
import { taskWorkbenchTargetKey } from '@/shared/state/workbench-selection'
import jyowoLogoUrl from '../../../../src-tauri/icons/jyowo-logo-concept-02.png?url'

const taskId = '01J00000000000000000000001'
const segmentId = '01J00000000000000000000002'
const diffBlobId = '01J00000000000000000000003'
const imageBlobId = '01J00000000000000000000004'
const fileBlobId = '01J00000000000000000000005'

const storyClient = {
  connect: async () => ({}) as never,
  loadTaskEvents: async () => ({ events: [], nextBeforeOffset: null, taskId }),
  readBlob: async (blobId: string) => {
    if (blobId === imageBlobId) {
      const bytes = new Uint8Array(await (await fetch(jyowoLogoUrl)).arrayBuffer())
      return {
        blobId,
        bytes,
        contentHash: Array.from({ length: 32 }, () => 2),
        mediaType: 'image/png',
        missing: false,
        size: bytes.byteLength,
      }
    }
    const text =
      blobId === fileBlobId
        ? 'pub fn replay_committed() {\n    assert_eq!(replayed, committed);\n}'
        : 'diff --git a/src/recovery.rs b/src/recovery.rs\n@@ -41,0 +42,1 @@\n+    assert_eq!(replayed, committed);'
    return {
      blobId,
      bytes: new TextEncoder().encode(text),
      contentHash: Array.from({ length: 32 }, () => 1),
      mediaType: blobId === fileBlobId ? 'text/plain' : 'text/x-diff',
      missing: false,
      size: text.length,
    }
  },
  request: async (request: ClientRequest) => ({
    message: {
      commandId:
        request.type === 'resolve_permission' || request.type === 'resolve_question'
          ? request.metadata.commandId
          : taskId,
      committedOffset: 4,
      streamVersion: 4,
      taskId,
      type: 'command_accepted' as const,
    },
    protocolVersion: 7,
  }),
}

const meta = {
  component: TaskWorkspaceView,
  decorators: [
    (Story) => (
      <main className="h-screen min-h-[600px] overflow-hidden bg-background px-4 pt-6 text-foreground sm:px-6">
        <Story />
      </main>
    ),
  ],
  parameters: {
    a11y: { test: 'error' },
    layout: 'fullscreen',
  },
  title: 'Tasks/Task workspace',
} satisfies Meta<typeof TaskWorkspaceView>

export default meta
type Story = StoryObj<typeof meta>

export const IdleTask: Story = workspaceStory(
  snapshot('idle', [
    item(1, 'user_message', 'Inspect scheduler recovery.'),
    item(2, 'assistant_text', 'Recovery is verified.'),
  ]),
)

export const Thinking: Story = workspaceStory(
  snapshot(
    'running',
    [
      item(1, 'user_message', 'Trace the journal replay path.', segmentId),
      item(2, 'notice', 'Run started', segmentId),
    ],
    {
      currentRun: {
        incompleteOutput: false,
        segmentId,
        startedAt: '2026-07-18T00:00:00Z',
        state: 'running',
      },
    },
  ),
)

export const Paused: Story = workspaceStory(
  snapshot('interrupted', [item(1, 'user_message', 'Trace the journal replay path.', segmentId)], {
    currentRun: {
      endedAt: '2026-07-18T00:00:01Z',
      incompleteOutput: false,
      segmentId,
      startedAt: '2026-07-18T00:00:00Z',
      state: 'interrupted',
      terminalReason: 'cancelled',
    },
  }),
)

export const ActiveStreaming: Story = workspaceStory(
  snapshot(
    'running',
    [
      item(1, 'user_message', 'Trace the journal replay path.'),
      item(2, 'notice', 'Run started', segmentId),
      item(3, 'assistant_text', 'Reading the committed event stream.', segmentId),
      item(4, 'tool_activity', 'Running recovery tests', segmentId, true),
      item(5, 'diff', '2 files changed, 18 insertions', segmentId, true, diffBlobId),
    ],
    {
      currentRun: {
        incompleteOutput: false,
        segmentId,
        startedAt: '2026-07-11T06:00:00Z',
        state: 'running',
      },
      queue: [queue(6, 'Review the recovery invariant'), queue(7, 'Run the release gate')],
    },
  ),
)

export const FileActivity: Story = workspaceStory(
  snapshot('completed', [
    item(1, 'user_message', '检查并修复恢复逻辑。'),
    toolItem(2, 'read', 'completed', 'FileRead', 'src/recovery.rs', 38),
    linkedArtifactItem(3, 'file', 'src/recovery.rs', 'tool-2', fileBlobId),
    toolItem(4, 'edit', 'completed', 'FileEdit', 'src/recovery.rs', 54),
    linkedArtifactItem(5, 'diff', 'src/recovery.rs', 'tool-4', diffBlobId),
    item(6, 'assistant_text', '恢复路径已经更新，并保留了当次文件快照。', segmentId),
  ]),
)

export const ReferenceProcessFlow: Story = {
  args: {
    client: storyClient,
    connectionState: 'connected',
    snapshot: referenceProcessSnapshot(),
  },
  render: (args) => <ReferenceProcessFixture {...args} />,
}

export const PermissionWaiting: Story = workspaceStory(
  snapshot(
    'waiting_permission',
    [
      item(1, 'user_message', 'Run the integration suite.'),
      item(2, 'notice', 'Run started', segmentId),
      item(3, 'permission', 'Permission required: execute integration test', segmentId),
    ],
    {
      currentRun: {
        incompleteOutput: false,
        segmentId,
        startedAt: '2026-07-11T06:00:00Z',
        state: 'waiting_permission',
      },
      pendingPermission: {
        details: {
          actionPlanHash: 'plan-hash',
          actorSource: { kind: 'engine' },
          expiresAt: '2026-07-11T07:00:00Z',
          kind: 'command',
          options: [
            { label: 'Allow once', optionId: 'allow_once' },
            { label: 'Deny', optionId: 'deny' },
          ],
          preview: 'cargo test -p jyowo-harness-daemon',
          sandboxPolicyHash: 'sandbox-hash',
          segmentId,
          subject: { command: 'cargo test -p jyowo-harness-daemon' },
          workspace: '/workspace',
        },
        requestId: '01J00000000000000000000009',
        revision: 1,
        route: 'foreground_task',
      },
    },
  ),
)

export const QuestionWaiting: Story = workspaceStory(
  snapshot(
    'waiting_input',
    [
      item(1, 'user_message', '为商城产品整理一份可供研发使用的 PRD。'),
      item(2, 'notice', 'Run started', segmentId),
    ],
    {
      currentRun: {
        incompleteOutput: false,
        segmentId,
        startedAt: '2026-07-18T00:00:00Z',
        state: 'waiting_input',
      },
      pendingQuestion: {
        expiresAt: '2026-07-18T01:00:00Z',
        questions: [
          {
            allowCustom: true,
            header: '需求范围',
            id: 'scope',
            multiSelect: false,
            options: [
              { id: 'product', label: '完整产品', description: '覆盖端到端产品能力' },
              { id: 'module', label: '单个模块', description: '只描述本次新增或改造范围' },
            ],
            question: '这份 PRD 需要覆盖什么范围？',
          },
          {
            allowCustom: true,
            header: '客户端',
            id: 'clients',
            multiSelect: true,
            options: [
              { id: 'mini-program', label: '微信小程序' },
              { id: 'mobile-web', label: 'H5 / 移动 Web' },
              { id: 'native', label: 'iOS / Android' },
            ],
            question: '需要覆盖哪些客户端？',
          },
          {
            allowCustom: false,
            header: '交付深度',
            id: 'depth',
            multiSelect: false,
            options: [
              { id: 'exploration', label: '方向探索', description: '确认目标、范围和核心流程' },
              { id: 'delivery', label: '开发就绪', description: '补齐状态、边界和验收标准' },
            ],
            question: '这次需要做到什么深度？',
          },
        ],
        requestId: '01J00000000000000000000011',
        revision: 1,
        segmentId,
        toolUseId: '01J00000000000000000000012',
      },
    },
  ),
)

export const FailedCommandLargeDiff: Story = workspaceStory(
  snapshot('failed', [
    item(1, 'user_message', 'Repair the daemon recovery path.'),
    item(2, 'notice', 'Run started', segmentId),
    item(3, 'command', 'cargo test -p jyowo-harness-daemon — exit code 1', segmentId),
    item(
      4,
      'diff',
      '8 files changed, 214 insertions, 63 deletions\n\nThe supervisor now restores task actors from committed projections and rejects stale commands without replaying indeterminate tools.',
      segmentId,
      false,
      diffBlobId,
    ),
    item(5, 'error', 'Command failed; inspect output before retrying', segmentId),
  ]),
)

export const InterruptedRecovery: Story = workspaceStory(
  snapshot('interrupted', [
    item(1, 'user_message', 'Continue after daemon restart.'),
    item(
      2,
      'tool_activity',
      'Package installation may have continued outside supervision',
      segmentId,
      true,
    ),
    item(3, 'assistant_text', 'Output preserved up to the last committed offset.', segmentId, true),
    item(
      4,
      'notice',
      'Run interrupted by restart; indeterminate tool will not replay',
      segmentId,
      true,
    ),
  ]),
)

export const OpenWorkbench: Story = workspaceStory(
  snapshot('completed', [
    item(1, 'user_message', 'Show the final changes.'),
    item(2, 'diff', '2 files changed, 18 insertions', segmentId, false, diffBlobId),
    item(3, 'file', 'recovery-report.md', segmentId, false, fileBlobId),
    item(4, 'assistant_text', 'The recovery invariant is covered.', segmentId),
  ]),
  [
    {
      blobId: diffBlobId,
      kind: 'diff',
      resourceId: diffBlobId,
      sourceEventId: 'event-2',
      taskId,
      title: '2 files changed, 18 insertions',
    },
    {
      blobId: fileBlobId,
      kind: 'file',
      resourceId: fileBlobId,
      sourceEventId: 'event-3',
      taskId,
      title: 'recovery-report.md',
    },
  ],
)

export const ObjectPreviews: Story = workspaceStory(
  snapshot('completed', [
    item(1, 'user_message', 'Review the generated assets.', undefined, false, fileBlobId),
    item(2, 'file', 'recovery-report.md', segmentId, false, fileBlobId),
    item(3, 'image', 'Jyowo application icon', segmentId, false, imageBlobId),
    item(4, 'assistant_text', 'The file and image are ready for inspection.', segmentId),
  ]),
  {
    blobId: imageBlobId,
    kind: 'source',
    resourceId: imageBlobId,
    sourceEventId: 'event-3',
    taskId,
    title: 'Jyowo application icon',
  },
)

export const ScrollFollowing: Story = {
  args: {
    client: storyClient,
    connectionState: 'connected',
    snapshot: null,
  },
  render: () => <ScrollFollowingFixture />,
}

function workspaceStory(
  storySnapshot: TaskSnapshot,
  initialTarget?: TaskWorkbenchTarget | TaskWorkbenchTarget[],
): Story {
  return {
    args: {
      client: storyClient,
      connectionState: 'connected',
      snapshot: storySnapshot,
    },
    render: (args) => <WorkspaceFixture {...args} initialTarget={initialTarget} />,
  }
}

function WorkspaceFixture({
  initialTarget,
  ...props
}: Parameters<typeof TaskWorkspaceView>[0] & {
  initialTarget?: TaskWorkbenchTarget | TaskWorkbenchTarget[]
}) {
  useEffect(() => {
    uiStore.setState({ taskWorkbenchByTaskId: {} })
    const targets = initialTarget
      ? Array.isArray(initialTarget)
        ? initialTarget
        : [initialTarget]
      : []
    targets.forEach((target, index) => {
      uiStore.getState().openTaskWorkbench(target)
      if (index < targets.length - 1) {
        uiStore
          .getState()
          .setTaskWorkbenchTabPinned(target.taskId, taskWorkbenchTargetKey(target), true)
      }
    })
    return () => {
      uiStore.setState({ taskWorkbenchByTaskId: {} })
    }
  }, [initialTarget])
  return <TaskWorkspaceView {...props} />
}

function ReferenceProcessFixture(props: Parameters<typeof TaskWorkspaceView>[0]) {
  useEffect(() => {
    const previousTheme = uiStore.getState().theme
    const previousLocale = appI18n.language
    uiStore.getState().setTheme('dark')
    void appI18n.changeLanguage('zh-CN')
    return () => {
      uiStore.getState().setTheme(previousTheme)
      void appI18n.changeLanguage(previousLocale)
    }
  }, [])
  return <TaskWorkspaceView {...props} />
}

function ScrollFollowingFixture() {
  const [timeline, setTimeline] = useState(() =>
    Array.from({ length: 32 }, (_, index) =>
      item(
        index + 1,
        index % 2 === 0 ? 'user_message' : 'assistant_text',
        `History message ${index + 1}. This line is long enough to make the timeline scroll.`,
      ),
    ),
  )

  useEffect(() => () => uiStore.getState().setTheme('system'), [])

  return (
    <div className="relative h-full">
      <div className="fixed top-2 left-2 z-50 flex gap-1" data-testid="scroll-story-controls">
        <button
          className="rounded border border-border bg-surface px-2 py-1 text-xs"
          onClick={() =>
            setTimeline((current) => [
              ...current,
              item(
                (current.at(-1)?.globalOffset ?? 0) + 1,
                'assistant_text',
                `New message ${current.length + 1}`,
              ),
            ])
          }
          type="button"
        >
          Append message
        </button>
        <button
          className="rounded border border-border bg-surface px-2 py-1 text-xs"
          onClick={() =>
            setTimeline((current) => {
              const last = current.at(-1)
              if (!last) return current
              return [
                ...current.slice(0, -1),
                { ...last, incomplete: true, summary: `${last.summary} streamed` },
              ]
            })
          }
          type="button"
        >
          Grow stream
        </button>
        <button
          className="rounded border border-border bg-surface px-2 py-1 text-xs"
          onClick={() => uiStore.getState().setTheme('light')}
          type="button"
        >
          Light theme
        </button>
        <button
          className="rounded border border-border bg-surface px-2 py-1 text-xs"
          onClick={() => uiStore.getState().setTheme('dark')}
          type="button"
        >
          Dark theme
        </button>
      </div>
      <TaskWorkspaceView
        client={storyClient}
        connectionState="connected"
        snapshot={snapshot('completed', timeline)}
      />
    </div>
  )
}

function snapshot(
  state: TaskProjection['state'],
  timeline: TimelineItemProjection[],
  overrides: Partial<TaskProjection> = {},
): TaskSnapshot {
  return {
    projection: {
      archived: false,
      lastGlobalOffset: timeline.at(-1)?.globalOffset ?? 0,
      queue: [],
      state,
      streamVersion: timeline.at(-1)?.globalOffset ?? 0,
      taskId,
      title: 'Verify daemon recovery',
      ...overrides,
    },
    snapshotOffset: timeline.at(-1)?.globalOffset ?? 0,
    timeline,
  }
}

function referenceProcessSnapshot() {
  return snapshot(
    'running',
    [
      item(1, 'user_message', '修复任务会话的过程展示。'),
      item(
        2,
        'assistant_text',
        '已开始处理会话区。先核对时间线投影、当前分支和工作区状态。',
        segmentId,
      ),
      toolItem(3, 'read', 'completed', 'read_file', 'timeline/TaskTimeline.tsx', 38),
      toolItem(
        4,
        'command',
        'completed',
        'exec_command',
        undefined,
        620,
        'git status --short',
        ' M apps/desktop/src/features/tasks/timeline/RunSegment.tsx',
      ),
      toolItem(
        5,
        'command',
        'completed',
        'exec_command',
        undefined,
        840,
        'rg -n "tool_activity" apps/desktop/src/features/tasks',
        "apps/desktop/src/features/tasks/timeline/RunSegment.tsx:31: item.kind === 'tool_activity'",
      ),
      item(
        6,
        'assistant_text',
        '现有问题来自生命周期事件占据正文，以及同一次工具调用被拆成多条记录。开始合并投影。',
        segmentId,
      ),
      toolItem(7, 'edit', 'completed', 'apply_patch', 'timeline/RunSegment.tsx', 54),
      toolItem(8, 'read', 'completed', 'read_file', 'task_projection.rs', 26),
      toolItem(
        9,
        'command',
        'completed',
        'exec_command',
        undefined,
        1_240,
        'pnpm -C apps/desktop test TaskTimeline',
        'Test Files  1 passed (1)\nTests  12 passed (12)',
      ),
      item(
        10,
        'diff',
        '4 files changed, 186 insertions, 74 deletions',
        segmentId,
        false,
        diffBlobId,
      ),
      item(11, 'image', 'task-context-reference.png', segmentId, false, imageBlobId),
      item(12, 'file', 'implementation-notes.md', segmentId, false, fileBlobId),
      item(
        13,
        'assistant_text',
        '工具调用已经按语义聚合。生命周期记录退出正文，运行状态改为单一入口。',
        segmentId,
      ),
      toolItem(14, 'command', 'running', 'exec_command', undefined, undefined, 'pnpm check'),
    ],
    {
      currentRun: {
        incompleteOutput: false,
        segmentId,
        startedAt: new Date(Date.now() - 40_000).toISOString(),
        state: 'running',
      },
      subagents: [
        {
          actorId: '01J00000000000000000000006',
          childTaskId: '01J00000000000000000000007',
          contextCursor: 3,
          delegationId: '01J00000000000000000000008',
          detached: false,
          parentSegmentId: segmentId,
          parentTaskId: taskId,
          segmentId: '01J00000000000000000000009',
          startedAt: new Date(Date.now() - 18_000).toISOString(),
          state: 'running',
          summary: '检查任务上下文视觉一致性',
        },
      ],
      workspace: { mode: 'current', root: '/Users/goya/Repo/Git/Jyowo' },
    },
  )
}

function item(
  globalOffset: number,
  kind: TimelineItemProjection['kind'],
  summary: string,
  runSegmentId?: string,
  incomplete = false,
  blobId?: string,
): TimelineItemProjection {
  return {
    blobId,
    globalOffset,
    id: `event-${globalOffset}`,
    incomplete,
    kind,
    runSegmentId,
    summary,
  }
}

function toolItem(
  globalOffset: number,
  operation: NonNullable<TimelineItemProjection['tool']>['operation'],
  status: NonNullable<TimelineItemProjection['tool']>['status'],
  toolName: string,
  subject?: string,
  durationMs?: number,
  command?: string,
  output?: string,
): TimelineItemProjection {
  return {
    ...item(globalOffset, 'tool_activity', toolName, segmentId, status !== 'completed'),
    tool: {
      durationMs,
      command,
      operation,
      output,
      status,
      subject,
      toolName,
      toolUseId: `tool-${globalOffset}`,
    },
  }
}

function linkedArtifactItem(
  globalOffset: number,
  kind: 'diff' | 'file',
  title: string,
  sourceToolUseId: string,
  blobId: string,
): TimelineItemProjection {
  return {
    ...item(globalOffset, kind, title, segmentId, false, blobId),
    contentBlocks: [
      {
        artifact: {
          artifactKind: kind,
          blobId,
          mediaType: 'text/plain',
          presentation: { preferredSurface: 'card' },
          sourceToolUseId,
          title,
        },
        type: 'artifact',
      },
    ],
  }
}

function queue(globalOffset: number, content: string): QueueItemProjection {
  return {
    attachments: [],
    content,
    contextReferences: [],
    createdAt: '2026-07-11T06:00:30Z',
    createdGlobalOffset: globalOffset,
    queueItemId: `01J000000000000000000000${globalOffset}`,
    revision: 1,
    state: 'queued',
  }
}
