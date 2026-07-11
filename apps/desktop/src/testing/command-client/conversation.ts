import type { RunEvent } from '@/shared/events/run-event-schema'
import type {
  ConversationEventBatchPayload,
  GetConversationResponse,
  ListActivityResponse,
  ListConversationsResponse,
  PageConversationWorktreeResponse,
  SubscribeConversationEventsResponse,
} from '@/shared/tauri/commands'
import {
  artifactRevision,
  assistantWork,
  fixtureRunModelSnapshot,
  permissionState,
} from '@/testing/conversation-worktree-builders'

import { timestamp } from './base'

export { fixtureRunModelSnapshot } from '@/testing/conversation-worktree-builders'

export const fixtureListConversations: ListConversationsResponse = {
  conversations: [
    {
      id: 'conversation-001',
      isEmpty: false,
      lastMessagePreview: 'Restore the product shell',
      title: 'Build the desktop foundation',
      updatedAt: timestamp,
    },
  ],
}

export const fixtureConversation: GetConversationResponse = {
  conversation: {
    id: 'conversation-001',
    messages: [
      {
        author: 'user',
        body: [
          "Let's scaffold the desktop app with Electron + React + TypeScript.",
          'Use Vite for the renderer. Keep it minimal and ready for local AI features.',
        ].join('\n'),
        id: 'message-001',
        timestamp: '2026-06-17T02:21:00.000Z',
      },
      {
        author: 'assistant',
        body: [
          'The runtime conversation is connected to the local workspace.',
          'Activity, artifacts, and context now come from command responses instead of embedded UI data.',
          'Continue from the composer to start another runtime-backed turn.',
        ].join('\n'),
        id: 'message-002',
        timestamp,
      },
    ],
    modelConfigId: null,
    title: 'Build the desktop foundation',
    updatedAt: timestamp,
  },
}

export const fixtureListActivity: ListActivityResponse = {
  events: [
    {
      id: 'evt-001',
      conversationSequence: 1,
      payload: {
        model: fixtureRunModelSnapshot,
        permissionMode: 'default',
        sessionId: 'conversation-001',
      },
      runId: 'run-001',
      sequence: 1,
      source: 'engine',
      timestamp,
      type: 'run.started',
      visibility: 'public',
    },
    {
      id: 'evt-002',
      conversationSequence: 2,
      payload: { toolUseId: 'start_run' },
      runId: 'run-001',
      sequence: 2,
      source: 'tool',
      timestamp,
      type: 'tool.approved',
      visibility: 'public',
    },
  ],
}

export const fixtureConversationWorktreePage: PageConversationWorktreeResponse = {
  turns: [
    {
      id: 'turn:message-001',
      conversationId: 'conversation-001',
      position: 0,
      user: {
        id: 'user:message-001',
        messageId: 'message-001',
        body: 'Restore the product shell',
        timestamp: '2026-06-17T02:21:00.000Z',
      },
      assistant: assistantWork({
        id: 'assistant:run-001',
        runId: 'run-001',
        model: fixtureRunModelSnapshot,
        status: 'running',
        segments: [
          {
            kind: 'process',
            id: 'segment:process:run-001',
            order: 0,
            status: 'running',
            summary: '正在处理请求',
            steps: [
              {
                id: 'process-step:run-001:reasoning',
                order: 0,
                kind: 'reasoning',
                status: 'running',
                title: '分析工作区状态',
                body: '正在检查本地项目上下文。',
              },
              {
                id: 'process-step:run-001:file-read',
                order: 1,
                kind: 'fileRead',
                status: 'complete',
                title: '读取项目文件',
                detail: {
                  type: 'activity',
                  summary: '已读取 1 个文件',
                  itemCount: 1,
                },
              },
            ],
          },
          {
            kind: 'text',
            id: 'segment:text:message-002',
            order: 1,
            messageId: 'message-002',
            body: 'I am checking the workspace state.',
          },
          {
            kind: 'toolGroup',
            id: 'segment:tools:tool-fixture-read',
            order: 2,
            attempts: [
              {
                id: 'tool:tool-fixture-read',
                order: 0,
                toolUseId: 'tool-fixture-read',
                toolName: 'read_file',
                status: 'completed',
                permission: permissionState({
                  id: 'permission:01HZ0000000000000000000001',
                  requestId: '01HZ0000000000000000000001',
                  toolUseId: 'tool-fixture-read',
                  status: 'approved',
                  reason: 'Approved once',
                }),
              },
              {
                id: 'tool:tool-fixture-verify',
                order: 1,
                toolUseId: 'tool-fixture-verify',
                toolName: 'local_verification',
                status: 'waitingPermission',
                permission: permissionState({
                  id: 'permission:01HZ0000000000000000000002',
                  requestId: '01HZ0000000000000000000002',
                  toolUseId: 'tool-fixture-verify',
                  status: 'pending',
                  reason: 'Awaiting approval',
                }),
              },
            ],
          },
        ],
      }),
    },
    {
      id: 'turn:message-003',
      conversationId: 'conversation-001',
      position: 1,
      user: {
        id: 'user:message-003',
        messageId: 'message-003',
        body: 'Run the checks',
        timestamp: '2026-06-17T02:22:00.000Z',
      },
      assistant: assistantWork({
        id: 'assistant:run-002',
        runId: 'run-002',
        model: fixtureRunModelSnapshot,
        status: 'complete',
        segments: [
          {
            kind: 'toolGroup',
            id: 'segment:tools:tool-fixture-test',
            order: 0,
            attempts: [
              {
                id: 'tool:tool-fixture-test',
                order: 0,
                toolUseId: 'tool-fixture-test',
                toolName: 'pnpm test',
                status: 'failed',
                failureSummary: '工具执行失败。可在详情中查看。',
              },
            ],
          },
          {
            kind: 'text',
            id: 'segment:text:message-004',
            order: 1,
            messageId: 'message-004',
            body: 'The checks need follow-up.',
          },
        ],
      }),
    },
  ],
  pageCursor: {
    turnId: 'turn:message-003',
    position: 1,
  },
  eventCursor: {
    eventId: '01ARZ3NDEKTSV4RRFFQ69G5FAV',
    conversationSequence: 9,
  },
  hasMoreBefore: false,
  hasMoreAfter: false,
  gap: false,
}

export function emptyWorktreePage(): PageConversationWorktreeResponse {
  return {
    turns: [],
    pageCursor: undefined,
    eventCursor: undefined,
    hasMoreBefore: false,
    hasMoreAfter: false,
    gap: false,
  }
}

export function worktreePageForFixtureRun(
  conversationId: string,
  prompt: string,
  clientMessageId: string | undefined,
  status: 'running' | 'complete',
): PageConversationWorktreeResponse {
  const turn: PageConversationWorktreeResponse['turns'][number] = {
    id: 'turn:message-fixture-user',
    conversationId,
    position: 0,
    user: {
      id: 'user:message-fixture-user',
      messageId: 'message-fixture-user',
      clientMessageId,
      body: prompt,
      timestamp,
    },
    assistant: assistantWork({
      id: 'assistant:run-001',
      runId: 'run-001',
      model: fixtureRunModelSnapshot,
      status,
      segments: [
        {
          kind: 'process',
          id: 'segment:process:run-001',
          order: 0,
          status,
          summary: status === 'running' ? '正在处理请求' : '已完成工作过程',
          steps: [
            {
              id: 'process-step:fixture-reasoning',
              order: 0,
              kind: 'reasoning',
              status,
              title: '整理实施计划',
              body: 'Drafting the implementation plan.',
            },
            {
              id: 'process-step:fixture-read',
              order: 1,
              kind: 'fileRead',
              status: 'complete',
              title: 'Reading files',
              detail: {
                type: 'activity',
                summary: 'Read project files',
                itemCount: 1,
              },
            },
          ],
        },
        {
          kind: 'toolGroup',
          id: 'segment:tools:tool-fixture-read',
          order: 1,
          attempts: [
            {
              id: 'tool:tool-fixture-read',
              order: 0,
              toolUseId: 'tool-fixture-read',
              toolName: 'Reading files',
              status: 'completed',
            },
            {
              id: 'tool:tool-fixture-verify',
              order: 1,
              toolUseId: 'tool-fixture-verify',
              toolName: 'Run local verification',
              status: status === 'running' ? 'waitingPermission' : 'completed',
              permission: permissionState({
                id: 'permission:01HZ0000000000000000000001',
                requestId: '01HZ0000000000000000000001',
                toolUseId: 'tool-fixture-verify',
                status: status === 'running' ? 'pending' : 'approved',
                reason:
                  status === 'running' ? 'Awaiting approval' : 'Approved for this verification run',
              }),
            },
          ],
        },
        {
          kind: 'artifact',
          id: 'segment:artifact:artifact-desktop-foundation',
          order: 2,
          artifactId: 'artifact-desktop-foundation',
          title: 'Desktop foundation created',
          revision: artifactRevision({
            artifactId: 'artifact-desktop-foundation',
            revisionId: 'revision-desktop-foundation-001',
            kind: 'code',
            sourceRunId: 'run-001',
            title: 'Desktop foundation created',
            contentRef: 'evidence-artifact-desktop-foundation',
          }),
        },
        {
          kind: 'text',
          id: 'segment:text:message-fixture-assistant',
          order: 3,
          messageId: 'message-fixture-assistant',
          body: 'The setup is ready for review.',
        },
      ],
    }),
  }

  return {
    turns: [turn],
    pageCursor: {
      turnId: turn.id,
      position: turn.position,
    },
    eventCursor: {
      eventId: '01ARZ3NDEKTSV4RRFFQ69G5FAV',
      conversationSequence: status === 'running' ? 7 : 9,
    },
    hasMoreBefore: false,
    hasMoreAfter: false,
    gap: false,
  }
}

export function fixtureTimelineEvent<TType extends RunEvent['type']>(
  type: TType,
  payload: Extract<RunEvent, { type: TType }>['payload'],
  options: Partial<RunEvent> = {},
): RunEvent {
  return {
    id: options.id ?? `evt-fixture-${type}`,
    conversationSequence: options.conversationSequence ?? 1,
    runId: options.runId ?? 'run-001',
    sequence: options.sequence ?? 1,
    source: options.source ?? 'engine',
    timestamp,
    type,
    visibility: options.visibility ?? 'public',
    payload,
  } as RunEvent
}

export type FixtureConversationEventState = {
  getListener: () => ((batch: ConversationEventBatchPayload) => void) | null
  getSubscription: () => SubscribeConversationEventsResponse | null
  trackTimeout: (timeoutId: number, resolve: () => void) => void
  untrackTimeout: (timeoutId: number) => void
}

export function emitFixtureConversationBatch(
  state: FixtureConversationEventState,
  subscription: SubscribeConversationEventsResponse | null,
  events: RunEvent[],
  delayMs = 0,
): Promise<void> {
  if (!state.getListener() || !subscription || events.length === 0) {
    return Promise.resolve()
  }

  return new Promise<void>((resolve) => {
    const timeoutId = window.setTimeout(() => {
      state.untrackTimeout(timeoutId)
      const listener = state.getListener()
      const currentSubscription = state.getSubscription()

      if (
        !listener ||
        currentSubscription !== subscription ||
        currentSubscription.subscriptionId !== subscription.subscriptionId ||
        currentSubscription.conversationId !== subscription.conversationId
      ) {
        resolve()
        return
      }

      listener({
        subscriptionId: currentSubscription.subscriptionId,
        conversationId: currentSubscription.conversationId,
        events,
        cursor: events.at(-1)
          ? {
              eventId: '01ARZ3NDEKTSV4RRFFQ69G5FAV',
              conversationSequence: events.at(-1)?.conversationSequence ?? 0,
            }
          : currentSubscription.cursor,
        gap: false,
        phase: 'live',
      })
      resolve()
    }, delayMs)
    state.trackTimeout(timeoutId, resolve)
  })
}
