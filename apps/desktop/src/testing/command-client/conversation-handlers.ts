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
  ListProjectConversationGroupsResponse,
  PageConversationWorktreeResponse,
  ProcessStep,
  StartRunResponse,
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
  | 'createDefaultConversation'
  | 'createProjectConversation'
  | 'deleteConversation'
  | 'deleteProjectConversation'
  | 'exportConversationEvidence'
  | 'getArtifactRevisionContent'
  | 'getConversation'
  | 'getConversationCommandOutput'
  | 'getConversationDiffPatch'
  | 'getConversationInspectorItem'
  | 'listActivity'
  | 'listConversations'
  | 'listProjectConversationGroups'
  | 'startRun'

export function createConversationCommandHandlers(
  state: TestCommandClientState,
): TestCommandHandlers<ConversationCommandKeys> {
  return {
    async createConversation() {
      await wait(state.options.delayMs)
      const conversation = createTestConversation(state)
      state.conversations = {
        conversations: [
          conversation,
          ...state.conversations.conversations.filter((current) => current.id !== conversation.id),
        ],
      }
      state.projectConversationGroups = addConversationToActiveProjectGroup(
        state.projectConversationGroups,
        conversation,
      )
      addConversationDetails(state, conversation)

      return {
        conversation,
      } satisfies CreateConversationResponse
    },
    async createDefaultConversation() {
      await wait(state.options.delayMs)
      const conversation = createTestConversation(state)
      state.conversations = {
        conversations: [
          conversation,
          ...state.conversations.conversations.filter((current) => current.id !== conversation.id),
        ],
      }
      addConversationDetails(state, conversation)

      return {
        conversation,
      } satisfies CreateConversationResponse
    },
    async createProjectConversation(path) {
      await wait(state.options.delayMs)
      const conversation = createTestConversation(state)
      state.projectConversationGroups = addConversationToProjectGroup(
        state.projectConversationGroups,
        path,
        conversation,
      )
      addConversationDetails(state, conversation)

      return {
        conversation,
      } satisfies CreateConversationResponse
    },
    async deleteConversation(conversationId) {
      await wait(state.options.delayMs)
      state.conversations = {
        conversations: state.conversations.conversations.filter(
          (conversation) => conversation.id !== conversationId,
        ),
      }
      state.projectConversationGroups = removeConversationFromActiveProjectGroup(
        state.projectConversationGroups,
        conversationId,
      )
      return {
        conversationId,
        status: 'deleted',
      } satisfies DeleteConversationResponse
    },
    async deleteProjectConversation(path, conversationId) {
      await wait(state.options.delayMs)
      state.projectConversationGroups = removeConversationFromProjectGroup(
        state.projectConversationGroups,
        path,
        conversationId,
      )
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
    async listActivity() {
      await wait(state.options.delayMs)
      return state.options.listActivity ?? fixtureListActivity
    },
    async listConversations() {
      await wait(state.options.delayMs)
      return state.conversations
    },
    async listProjectConversationGroups() {
      await wait(state.options.delayMs)
      return state.projectConversationGroups
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
            actionPlanHash: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
            autoResolved: false,
            decisionScope: 'this run',
            effectiveMode: request.permissionMode ?? 'default',
            exposure: 'workspace',
            operation: 'Run local verification',
            reason: 'Confirm the generated foundation before continuing.',
            review: {
              confirmation: { type: 'none' },
              details: [],
              redacted: true,
              summary: 'Permission review unavailable.',
            },
            requestId: '01HZ0000000000000000000001',
            sandboxPolicy: {
              mode: { osLevel: 'none' },
              network: 'none',
              resourceLimits: {
                maxCpuCores: null,
                maxMemoryBytes: null,
                maxOpenFiles: null,
                maxPids: null,
                maxWallClockMs: null,
              },
              scope: 'workspace_only',
            },
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
  }
}

function addConversationToActiveProjectGroup(
  current: ListProjectConversationGroupsResponse,
  conversation: CreateConversationResponse['conversation'],
): ListProjectConversationGroupsResponse {
  if (!current.activePath) {
    return current
  }

  return addConversationToProjectGroup(current, current.activePath, conversation)
}

function addConversationToProjectGroup(
  current: ListProjectConversationGroupsResponse,
  projectPath: string,
  conversation: CreateConversationResponse['conversation'],
): ListProjectConversationGroupsResponse {
  return {
    ...current,
    groups: current.groups.map((group) =>
      group.project.path === projectPath
        ? {
            ...group,
            conversations: [
              conversation,
              ...group.conversations.filter((current) => current.id !== conversation.id),
            ],
          }
        : group,
    ),
  }
}

function removeConversationFromActiveProjectGroup(
  current: ListProjectConversationGroupsResponse,
  conversationId: string,
): ListProjectConversationGroupsResponse {
  if (!current.activePath) {
    return current
  }

  return removeConversationFromProjectGroup(current, current.activePath, conversationId)
}

function removeConversationFromProjectGroup(
  current: ListProjectConversationGroupsResponse,
  projectPath: string,
  conversationId: string,
): ListProjectConversationGroupsResponse {
  return {
    ...current,
    groups: current.groups.map((group) =>
      group.project.path === projectPath
        ? {
            ...group,
            conversations: group.conversations.filter(
              (conversation) => conversation.id !== conversationId,
            ),
          }
        : group,
    ),
  }
}

function createTestConversation(
  state: TestCommandClientState,
): CreateConversationResponse['conversation'] {
  state.createdConversationCounter += 1
  return {
    id: `conversation-created-${String(state.createdConversationCounter).padStart(3, '0')}`,
    isEmpty: true,
    lastMessagePreview: 'Start from the composer when ready.',
    title: 'New conversation',
    updatedAt: new Date().toISOString(),
  }
}

function addConversationDetails(
  state: TestCommandClientState,
  conversation: CreateConversationResponse['conversation'],
) {
  state.conversationDetailsById.set(conversation.id, {
    conversation: {
      id: conversation.id,
      messages: [],
      modelConfigId: null,
      title: conversation.title,
      updatedAt: conversation.updatedAt,
    },
  })
  state.worktreePagesByConversation.set(conversation.id, emptyWorktreePage())
}
