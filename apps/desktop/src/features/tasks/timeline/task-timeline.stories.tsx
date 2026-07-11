import type { Meta, StoryObj } from '@storybook/react-vite'
import { useEffect } from 'react'
import { TaskWorkspaceView } from '@/features/tasks/TaskWorkspace'
import type { TaskSnapshot } from '@/features/tasks/task-store'
import type {
  ClientRequest,
  QueueItemProjection,
  TaskProjection,
  TimelineItemProjection,
} from '@/generated/daemon-protocol'
import { uiStore } from '@/shared/state/ui-store'

const taskId = '01J00000000000000000000001'
const segmentId = '01J00000000000000000000002'
const diffBlobId = '01J00000000000000000000003'

const storyClient = {
  connect: async () => ({}) as never,
  readBlob: async () => ({
    blobId: diffBlobId,
    bytes: new TextEncoder().encode(
      'diff --git a/src/recovery.rs b/src/recovery.rs\n+assert_eq!(replayed, committed);',
    ),
    contentHash: Array.from({ length: 32 }, () => 1),
    mediaType: 'text/x-diff',
    missing: false,
    size: 87,
  }),
  request: async (request: ClientRequest) => ({
    message: {
      commandId: request.type === 'resolve_permission' ? request.metadata.commandId : taskId,
      committedOffset: 4,
      streamVersion: 4,
      taskId,
      type: 'command_accepted' as const,
    },
    protocolVersion: 1,
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
    item(3, 'assistant_text', 'The recovery invariant is covered.', segmentId),
  ]),
  true,
)

function workspaceStory(storySnapshot: TaskSnapshot, workbench = false): Story {
  return {
    args: {
      client: storyClient,
      connectionState: 'connected',
      snapshot: storySnapshot,
    },
    render: (args) => <WorkspaceFixture {...args} workbench={workbench} />,
  }
}

function WorkspaceFixture({
  workbench,
  ...props
}: Parameters<typeof TaskWorkspaceView>[0] & { workbench: boolean }) {
  useEffect(() => {
    uiStore.setState(
      workbench
        ? {
            taskWorkbenchMode: 'inspector',
            taskWorkbenchSelection: {
              blobId: diffBlobId,
              eventId: 'event-2',
              panel: 'changes',
              segmentId,
              taskId,
            },
          }
        : { taskWorkbenchMode: 'closed', taskWorkbenchSelection: null },
    )
    return () => {
      uiStore.setState({ taskWorkbenchMode: 'closed', taskWorkbenchSelection: null })
    }
  }, [workbench])
  return <TaskWorkspaceView {...props} />
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
