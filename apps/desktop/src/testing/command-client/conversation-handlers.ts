import type {
  ArtifactSegment,
  ConversationInspectorSelection,
  ConversationTurn,
  CreateConversationResponse,
  DeleteConversationResponse,
  ExportConversationEvidenceResponse,
  GetArtifactRevisionContentResponse,
  GetConversationCommandOutputResponse,
  GetConversationDiffPatchResponse,
  GetConversationInspectorItemResponse,
  PageConversationWorktreeResponse,
  ProcessStep,
  ResolvePermissionResponse,
  StartRunResponse,
  UnsubscribeConversationEventsResponse,
} from '@/shared/tauri/commands'

import { resolveResponseOverride, wait } from './base'
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

const fixtureEvidenceContentHash = 'b'.repeat(64)

function inspectorItemFromWorktreePage(
  page: PageConversationWorktreeResponse,
  selection: ConversationInspectorSelection,
): GetConversationInspectorItemResponse {
  if (selection.kind === 'turn') {
    const turn = page.turns.find((turn) => turn.id === selection.turnId)
    return turn ? { item: { kind: 'turn', turn } } : { item: { kind: 'empty' } }
  }

  for (const turn of page.turns) {
    const item = inspectorItemFromTurn(turn, selection)
    if (item.item.kind !== 'empty') {
      return item
    }
  }

  return { item: { kind: 'empty' } }
}

function inspectorItemFromTurn(
  turn: ConversationTurn,
  selection: ConversationInspectorSelection,
): GetConversationInspectorItemResponse {
  for (const segment of turn.assistant?.segments ?? []) {
    if (segment.kind === 'toolGroup') {
      for (const attempt of segment.attempts) {
        if (selection.kind === 'tool' && attempt.toolUseId === selection.toolUseId) {
          return { item: { kind: 'tool', attempt } }
        }
        if (
          selection.kind === 'decision' &&
          attempt.permission?.requestId === selection.requestId
        ) {
          return { item: { kind: 'decision', decision: attempt.permission } }
        }
      }
    }
    if (segment.kind === 'process') {
      for (const step of segment.steps ?? []) {
        const item = inspectorItemFromProcessStep(step, selection)
        if (item.item.kind !== 'empty') {
          return item
        }
      }
    }
    if (segment.kind === 'artifact' && artifactSegmentMatches(selection, segment)) {
      return { item: { kind: 'artifact', segment } }
    }
  }

  return { item: { kind: 'empty' } }
}

function inspectorItemFromProcessStep(
  step: ProcessStep,
  selection: ConversationInspectorSelection,
): GetConversationInspectorItemResponse {
  const detail = step.detail
  if (detail?.type === 'command' && commandMatches(selection, step)) {
    return { item: { kind: 'command', command: detail } }
  }
  if (detail?.type === 'diff' && changeSetMatches(selection, detail)) {
    return { item: { kind: 'diff', changeSet: detail } }
  }
  return { item: { kind: 'empty' } }
}

function commandMatches(selection: ConversationInspectorSelection, step: ProcessStep) {
  if (step.detail?.type !== 'command') {
    return false
  }
  if (selection.kind === 'command') {
    return (
      (selection.fullOutputRef !== undefined &&
        step.detail.fullOutputRef === selection.fullOutputRef) ||
      (selection.eventId !== undefined &&
        step.eventRefs?.some((eventRef) => eventRef.eventId === selection.eventId))
    )
  }
  return selection.kind === 'evidenceRef' && step.detail.fullOutputRef === selection.evidenceRefId
}

function changeSetMatches(
  selection: ConversationInspectorSelection,
  changeSet: Extract<ProcessStep['detail'], { type: 'diff' }>,
) {
  if (selection.kind === 'diff') {
    return changeSet.id === selection.changeSetId
  }
  return (
    selection.kind === 'evidenceRef' &&
    changeSet.files.some((file) => file.fullPatchRef === selection.evidenceRefId)
  )
}

function artifactSegmentMatches(
  selection: ConversationInspectorSelection,
  segment: ArtifactSegment,
) {
  if (selection.kind === 'artifact') {
    return segment.artifactId === selection.artifactId
  }
  if (selection.kind === 'artifactRevision') {
    return (
      segment.revision.revisionId === selection.revisionId &&
      (selection.artifactId === undefined || segment.artifactId === selection.artifactId)
    )
  }
  return (
    selection.kind === 'evidenceRef' &&
    (segment.revision.contentRef === selection.evidenceRefId ||
      segment.revision.previewRef === selection.evidenceRefId)
  )
}

type ConversationCommandKeys =
  | 'createConversation'
  | 'deleteConversation'
  | 'exportConversationEvidence'
  | 'getArtifactRevisionContent'
  | 'getConversation'
  | 'getConversationCommandOutput'
  | 'getConversationDiffPatch'
  | 'getConversationInspectorItem'
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
    async getConversationCommandOutput(request) {
      await wait(state.options.delayMs)
      return resolveResponseOverride(
        state.options.conversationCommandOutput,
        {
          refId: request.fullOutputRef,
          kind: 'command-output',
          output: 'fixture command output',
          contentType: 'text/plain; charset=utf-8',
          byteLength: 22,
          contentBytes: 22,
          offsetBytes: 0,
          limitBytes: 65_536,
          totalBytes: 22,
          returnedBytes: 22,
          maxBytes: 65_536,
          truncated: false,
          hasMore: false,
          contentHash: fixtureEvidenceContentHash,
          hashAlgorithm: 'blake3',
          redactionState: 'clean',
        } satisfies GetConversationCommandOutputResponse,
        request,
      )
    },
    async getConversationDiffPatch(request) {
      await wait(state.options.delayMs)
      return resolveResponseOverride(
        state.options.conversationDiffPatch,
        {
          refId: request.fullPatchRef,
          kind: 'diff-patch',
          patch: 'diff --git a/file.ts b/file.ts\n+fixture patch\n',
          contentType: 'text/x-diff; charset=utf-8',
          byteLength: 44,
          contentBytes: 44,
          offsetBytes: 0,
          limitBytes: 65_536,
          totalBytes: 44,
          returnedBytes: 44,
          maxBytes: 65_536,
          truncated: false,
          hasMore: false,
          contentHash: fixtureEvidenceContentHash,
          hashAlgorithm: 'blake3',
          redactionState: 'clean',
        } satisfies GetConversationDiffPatchResponse,
        request,
      )
    },
    async getArtifactRevisionContent(request) {
      await wait(state.options.delayMs)
      return resolveResponseOverride(
        state.options.artifactRevisionContent,
        {
          refId: request.contentRef,
          kind: 'artifact-content',
          content: 'fixture artifact content',
          contentType: 'text/plain; charset=utf-8',
          byteLength: 24,
          contentBytes: 24,
          offsetBytes: 0,
          limitBytes: 65_536,
          totalBytes: 24,
          returnedBytes: 24,
          maxBytes: 65_536,
          truncated: false,
          hasMore: false,
          contentHash: fixtureEvidenceContentHash,
          hashAlgorithm: 'blake3',
          redactionState: 'clean',
          artifactId: 'artifact-fixture',
          revisionId: 'revision-fixture',
        } satisfies GetArtifactRevisionContentResponse,
        request,
      )
    },
    async exportConversationEvidence(request) {
      await wait(state.options.delayMs)
      return resolveResponseOverride(
        state.options.conversationEvidenceExport,
        {
          refId: request.refId,
          kind: request.kind,
          contentType: 'text/plain; charset=utf-8',
          byteLength: 22,
          exportedAt: '2026-06-17T02:22:00.000Z',
          path: `.jyowo/runtime/exports/evidence-${request.kind}-fixture.txt`,
        } satisfies ExportConversationEvidenceResponse,
        request,
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
    async getConversationInspectorItem(request) {
      await wait(state.options.delayMs)
      const page =
        state.options.conversationWorktreePage ??
        state.worktreePagesByConversation.get(request.conversationId) ??
        emptyWorktreePage()
      return resolveResponseOverride(
        state.options.conversationInspectorItem,
        inspectorItemFromWorktreePage(page, request.selection),
        request,
      )
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
