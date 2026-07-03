import type {
  CreateConversationResponse,
  DeleteConversationResponse,
  ResolvePermissionResponse,
  StartRunResponse,
  UnsubscribeConversationEventsResponse,
} from '@/shared/tauri/commands'

import { wait } from './base'
import {
  emitFixtureConversationBatch,
  emptyWorktreePage,
  fixtureConversation,
  fixtureListActivity,
  fixtureRunModelSnapshot,
  fixtureTimelineEvent,
  worktreePageForFixtureRun,
} from './conversation'
import type { TestCommandClientState, TestCommandHandlers } from './state'

type ConversationCommandKeys =
  | 'createConversation'
  | 'deleteConversation'
  | 'getConversation'
  | 'listenConversationEventBatches'
  | 'listActivity'
  | 'listConversations'
  | 'pageConversationTimeline'
  | 'pageConversationWorktree'
  | 'resolvePermission'
  | 'startRun'
  | 'subscribeConversationEvents'
  | 'unsubscribeConversationEvents'

export function createConversationCommandHandlers(
  state: TestCommandClientState,
): TestCommandHandlers<ConversationCommandKeys> {
  return {
    async createConversation() {
      await wait(state.options.delayMs)
      state.createdConversationCounter += 1
      const conversationId = `conversation-created-${String(state.createdConversationCounter).padStart(3, '0')}`
      const conversation = {
        id: conversationId,
        isEmpty: true,
        lastMessagePreview: 'Start from the composer when ready.',
        title: 'New conversation',
        updatedAt: new Date().toISOString(),
      } satisfies CreateConversationResponse['conversation']
      state.conversations = {
        conversations: [
          conversation,
          ...state.conversations.conversations.filter((current) => current.id !== conversationId),
        ],
      }
      state.conversationDetailsById.set(conversationId, {
        conversation: {
          id: conversationId,
          messages: [],
          modelConfigId: null,
          title: conversation.title,
          updatedAt: conversation.updatedAt,
        },
      })
      state.worktreePagesByConversation.set(conversationId, emptyWorktreePage())

      return {
        conversation,
      } satisfies CreateConversationResponse
    },
    async deleteConversation(conversationId) {
      await wait(state.options.delayMs)
      return {
        conversationId,
        status: 'deleted',
      } satisfies DeleteConversationResponse
    },
    async getConversation(conversationId) {
      await wait(state.options.delayMs)
      return (
        state.options.conversation ??
        state.conversationDetailsById.get(conversationId) ??
        fixtureConversation
      )
    },
    async listenConversationEventBatches(onBatch) {
      await wait(state.options.delayMs)
      state.batchListener = onBatch
      return () => {
        if (state.batchListener === onBatch) {
          state.batchListener = null
          state.clearPendingBatches()
        }
      }
    },
    async listActivity() {
      await wait(state.options.delayMs)
      return state.options.listActivity ?? fixtureListActivity
    },
    async listConversations() {
      await wait(state.options.delayMs)
      return state.conversations
    },
    async pageConversationTimeline(request) {
      await wait(state.options.delayMs)
      const page = state.options.conversationTimelinePage ?? {
        events: [],
        cursor: undefined,
        gap: false,
      }
      if (!request.afterCursor) {
        return page
      }

      const afterSequence = request.afterCursor.conversationSequence
      return {
        ...page,
        events: page.events.filter((event) => event.conversationSequence > afterSequence),
      }
    },
    async pageConversationWorktree(request) {
      await wait(state.options.delayMs)
      const page =
        state.options.conversationWorktreePage ??
        state.worktreePagesByConversation.get(request.conversationId) ??
        emptyWorktreePage()
      if (!request.pageCursor) {
        return page
      }

      const pageCursor = request.pageCursor
      return {
        ...page,
        turns: page.turns.filter((turn) =>
          request.direction === 'before'
            ? turn.position < pageCursor.position
            : turn.position > pageCursor.position,
        ),
      }
    },
    async resolvePermission(request) {
      await wait(state.options.delayMs)
      await state.completionBatchFlushed
      emitFixtureConversationBatch(
        state.fixtureEventState,
        state.activeSubscription,
        [
          fixtureTimelineEvent(
            'permission.resolved',
            {
              autoResolved: false,
              decision: request.decision,
              requestId: request.requestId,
            },
            {
              conversationSequence: 10,
              id: 'evt-fixture-permission-resolved',
              sequence: 10,
              source: 'policy',
            },
          ),
        ],
        120,
      )
      return {
        ...request,
        status: 'resolved',
      } satisfies ResolvePermissionResponse
    },
    async startRun(request) {
      await wait(state.options.delayMs)
      state.worktreePagesByConversation.set(
        request.conversationId,
        worktreePageForFixtureRun(
          request.conversationId,
          request.prompt,
          request.clientMessageId,
          'running',
        ),
      )
      emitFixtureConversationBatch(state.fixtureEventState, state.activeSubscription, [
        fixtureTimelineEvent(
          'run.started',
          {
            model: fixtureRunModelSnapshot,
            permissionMode: request.permissionMode ?? 'default',
            sessionId: request.conversationId,
          },
          { conversationSequence: 1, id: 'evt-fixture-run-started', sequence: 1 },
        ),
        fixtureTimelineEvent(
          'user.message.appended',
          {
            body: request.prompt,
            clientMessageId: request.clientMessageId,
            messageId: 'message-fixture-user',
          },
          {
            conversationSequence: 2,
            id: 'evt-fixture-user-message',
            sequence: 2,
            source: 'user',
          },
        ),
        fixtureTimelineEvent(
          'assistant.delta',
          {
            messageId: 'message-fixture-delta',
            text: 'Drafting the implementation plan.',
          },
          {
            conversationSequence: 3,
            id: 'evt-fixture-assistant-delta',
            sequence: 3,
            source: 'assistant',
          },
        ),
        fixtureTimelineEvent(
          'tool.requested',
          {
            argumentsSummary: 'Input withheld from conversation timeline.',
            toolName: 'read_file',
            toolUseId: 'tool-fixture-read',
          },
          {
            conversationSequence: 4,
            id: 'evt-fixture-tool-requested',
            sequence: 4,
            source: 'tool',
          },
        ),
        fixtureTimelineEvent(
          'tool.completed',
          {
            durationMs: 42,
            outputSummary: 'Output withheld from conversation timeline.',
            toolUseId: 'tool-fixture-read',
          },
          {
            conversationSequence: 5,
            id: 'evt-fixture-tool-completed',
            sequence: 5,
            source: 'tool',
          },
        ),
        fixtureTimelineEvent(
          'permission.requested',
          {
            actorSource: { type: 'parentRun' },
            autoResolved: false,
            decisionScope: 'this run',
            exposure: 'workspace',
            operation: 'Run local verification',
            reason: 'Confirm the generated foundation before continuing.',
            requestId: '01HZ0000000000000000000001',
            severity: 'medium',
            target: 'local verification task',
            toolUseId: 'tool-fixture-read',
            workspaceBoundary: 'workspace',
          },
          {
            conversationSequence: 6,
            id: 'evt-fixture-permission-requested',
            sequence: 6,
            source: 'policy',
          },
        ),
        fixtureTimelineEvent(
          'artifact.created',
          { artifactId: 'artifact-desktop-foundation', status: 'ready' },
          {
            conversationSequence: 7,
            id: 'evt-fixture-artifact-created',
            sequence: 7,
            source: 'engine',
          },
        ),
      ])
      state.worktreePagesByConversation.set(
        request.conversationId,
        worktreePageForFixtureRun(
          request.conversationId,
          request.prompt,
          request.clientMessageId,
          'complete',
        ),
      )
      state.completionBatchFlushed = emitFixtureConversationBatch(
        state.fixtureEventState,
        state.activeSubscription,
        [
          fixtureTimelineEvent(
            'assistant.completed',
            {
              body: 'The setup is ready for review.',
              messageId: 'message-fixture-assistant',
            },
            {
              conversationSequence: 8,
              id: 'evt-fixture-assistant-completed',
              sequence: 8,
              source: 'assistant',
            },
          ),
          fixtureTimelineEvent(
            'run.ended',
            { reason: 'completed' },
            {
              conversationSequence: 9,
              id: 'evt-fixture-run-ended',
              sequence: 9,
            },
          ),
        ],
        100,
      )
      return { runId: 'run-001', status: 'started' } satisfies StartRunResponse
    },
    async subscribeConversationEvents(request) {
      await wait(state.options.delayMs)
      state.subscriptionCounter += 1
      state.activeSubscription = state.options.subscribeConversationEvents ?? {
        subscriptionId: `subscription-fixture-${state.subscriptionCounter}`,
        conversationId: request.conversationId,
        replayEvents: [],
        gap: false,
      }
      return state.activeSubscription
    },
    async unsubscribeConversationEvents(subscriptionId) {
      await wait(state.options.delayMs)
      if (state.activeSubscription?.subscriptionId === subscriptionId) {
        state.activeSubscription = null
        state.clearPendingBatches()
      }
      return {
        subscriptionId,
        status: 'unsubscribed',
      } satisfies UnsubscribeConversationEventsResponse
    },
  }
}
