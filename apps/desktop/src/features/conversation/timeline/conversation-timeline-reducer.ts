import type { ConversationCursor } from '@/shared/tauri/commands'
import type {
  ArtifactBlock,
  ArtifactView,
  ConversationBlock,
  ConversationSnapshot,
  PermissionRequestBlock,
  TimelineRunEvent,
  ToolGroupBlock,
  ToolItem,
  UserMessageBlock,
} from './conversation-blocks'
import type { ConversationTimelineAction } from './conversation-timeline-actions'
import {
  addBlock,
  appendAssistantAnswerDelta,
  finalizeAssistantMessage,
  findBlockById,
  nextSequence,
  patchBlock,
  reconcileSnapshotMessages,
  selectBlocksFromState,
  sortBlocks,
} from './conversation-timeline-index'
import { appendThinkingDelta, removeThinkingBlocksForRun } from './conversation-timeline-thinking'

export type ConversationTimelineState = {
  conversationId: string
  blockOrder: string[]
  blocksById: Record<string, ConversationBlock>
  eventIds: Record<string, true>
  cursor: ConversationCursor | null
  activeRunIds: string[]
  activeTurnByRunId: Record<string, string>
  clientMessageByRunId: Record<string, string>
  optimisticBlocksByClientMessageId: Record<string, string>
  streamingBlockByRunId: Record<string, string>
  toolGroupBlockByRunId: Record<string, string>
  artifactBlockByArtifactId: Record<string, string>
  permissionBlockByRequestId: Record<string, string>
  assistantBlockByMessageId: Record<string, string>
  thinkingBlockByRunId: Record<string, string>
  pendingAssistantReconcileByMessageId: Record<string, true>
  lastConversationEventSequence: number | null
  gapRecoverySequence: number | null
  hasGap: boolean
}

export function createConversationTimelineState(conversationId: string): ConversationTimelineState {
  return {
    conversationId,
    blockOrder: [],
    blocksById: {},
    eventIds: {},
    cursor: null,
    activeRunIds: [],
    activeTurnByRunId: {},
    clientMessageByRunId: {},
    optimisticBlocksByClientMessageId: {},
    streamingBlockByRunId: {},
    toolGroupBlockByRunId: {},
    artifactBlockByArtifactId: {},
    permissionBlockByRequestId: {},
    assistantBlockByMessageId: {},
    thinkingBlockByRunId: {},
    pendingAssistantReconcileByMessageId: {},
    lastConversationEventSequence: null,
    gapRecoverySequence: null,
    hasGap: false,
  }
}

export function conversationTimelineReducer(
  state: ConversationTimelineState,
  action: ConversationTimelineAction,
): ConversationTimelineState {
  switch (action.type) {
    case 'hydrateSnapshot':
      return hydrateSnapshot(state, action.snapshot)
    case 'applyEvents':
      return applyEvents(cloneTimelineState(state), action.events, action.cursor ?? null)
    case 'applyArtifacts':
      return applyArtifacts(cloneTimelineState(state), action.artifacts)
    case 'localSubmit':
      return localSubmit(cloneTimelineState(state), action)
    case 'commandAccepted':
      return commandAccepted(cloneTimelineState(state), action.clientMessageId, action.runId)
    case 'commandFailed':
      return patchUserBlock(cloneTimelineState(state), action.clientMessageId, {
        status: 'failed',
        errorMessage: action.errorMessage,
      })
    case 'assistantFinalContentMissing':
      return markAssistantReconcile(cloneTimelineState(state), action.runId, action.messageId)
    case 'snapshotReconciled':
      return snapshotReconciled(cloneTimelineState(state), action.snapshot)
    case 'permissionSubmitting':
      return patchPermissionBlock(cloneTimelineState(state), action.requestId, {
        status: 'submitting',
        submitDecision: action.decision,
      })
    case 'permissionSubmitFailed':
      return patchPermissionBlock(cloneTimelineState(state), action.requestId, {
        status: 'failed',
        errorMessage: action.errorMessage,
      })
    case 'markGap':
      return {
        ...state,
        cursor: action.afterCursor ?? state.cursor,
        hasGap: true,
      }
  }
}

function cloneTimelineState(state: ConversationTimelineState): ConversationTimelineState {
  return {
    ...state,
    blockOrder: [...state.blockOrder],
    blocksById: { ...state.blocksById },
    eventIds: { ...state.eventIds },
  }
}

function hydrateSnapshot(
  state: ConversationTimelineState,
  snapshot: ConversationSnapshot,
): ConversationTimelineState {
  const next = {
    ...cloneTimelineState(state),
    activeRunIds: [...state.activeRunIds],
    activeTurnByRunId: { ...state.activeTurnByRunId },
    clientMessageByRunId: { ...state.clientMessageByRunId },
    optimisticBlocksByClientMessageId: { ...state.optimisticBlocksByClientMessageId },
    streamingBlockByRunId: { ...state.streamingBlockByRunId },
    toolGroupBlockByRunId: { ...state.toolGroupBlockByRunId },
    artifactBlockByArtifactId: { ...state.artifactBlockByArtifactId },
    permissionBlockByRequestId: { ...state.permissionBlockByRequestId },
    assistantBlockByMessageId: { ...state.assistantBlockByMessageId },
    thinkingBlockByRunId: { ...state.thinkingBlockByRunId },
    pendingAssistantReconcileByMessageId: { ...state.pendingAssistantReconcileByMessageId },
  }

  reconcileSnapshotMessages(next, snapshot)
  snapshotReconciled(next, snapshot)
  sortBlocks(next)
  return next
}

function applyEvents(
  state: ConversationTimelineState,
  events: TimelineRunEvent[],
  cursor: ConversationCursor | null,
): ConversationTimelineState {
  let next = {
    ...state,
    activeRunIds: [...state.activeRunIds],
    eventIds: { ...state.eventIds },
    activeTurnByRunId: { ...state.activeTurnByRunId },
    clientMessageByRunId: { ...state.clientMessageByRunId },
    optimisticBlocksByClientMessageId: { ...state.optimisticBlocksByClientMessageId },
    streamingBlockByRunId: { ...state.streamingBlockByRunId },
    toolGroupBlockByRunId: { ...state.toolGroupBlockByRunId },
    artifactBlockByArtifactId: { ...state.artifactBlockByArtifactId },
    permissionBlockByRequestId: { ...state.permissionBlockByRequestId },
    assistantBlockByMessageId: { ...state.assistantBlockByMessageId },
    thinkingBlockByRunId: { ...state.thinkingBlockByRunId },
    pendingAssistantReconcileByMessageId: { ...state.pendingAssistantReconcileByMessageId },
    lastConversationEventSequence: state.lastConversationEventSequence,
    gapRecoverySequence: state.gapRecoverySequence,
  }
  let detectedGap = false
  let appliedEvent = false

  for (const event of events) {
    if (next.eventIds[event.id]) {
      continue
    }
    if (
      next.lastConversationEventSequence !== null &&
      event.conversationSequence <= next.lastConversationEventSequence
    ) {
      next.hasGap = true
      detectedGap = true
      continue
    }
    if (
      next.lastConversationEventSequence !== null &&
      event.conversationSequence > next.lastConversationEventSequence + 1
    ) {
      next.hasGap = true
      next.gapRecoverySequence = Math.max(
        next.gapRecoverySequence ?? event.conversationSequence,
        event.conversationSequence,
      )
      detectedGap = true
      continue
    }
    next.eventIds[event.id] = true
    next.lastConversationEventSequence = event.conversationSequence
    next = applyEvent(next, event)
    appliedEvent = true
  }

  if (!detectedGap && (!next.hasGap || appliedEvent || next.gapRecoverySequence === null)) {
    if (
      next.gapRecoverySequence !== null &&
      (next.lastConversationEventSequence ?? 0) < next.gapRecoverySequence
    ) {
      next.hasGap = true
      return next
    }
    next.hasGap = false
    next.gapRecoverySequence = null
    next.cursor = cursor ?? next.cursor
  }
  return next
}

function applyEvent(
  state: ConversationTimelineState,
  event: TimelineRunEvent,
): ConversationTimelineState {
  if (event.visibility === 'withheld') {
    addBlock(state, {
      ...baseBlock(state, event, `withheld:${event.id}`),
      kind: 'systemNotice',
      message: 'Event details are withheld.',
      tone: 'warning',
    })
    return state
  }

  switch (event.type) {
    case 'run.started':
      state.activeRunIds = addUnique(state.activeRunIds, event.runId)
      return state
    case 'run.ended':
      state.activeRunIds = state.activeRunIds.filter((runId) => runId !== event.runId)
      removeThinkingBlocksForRun(state, event.runId)
      finalizeStreamingFromRunEnd(state, event.runId)
      return state
    case 'user.message.appended':
      return applyUserMessageAppended(state, event)
    case 'assistant.delta':
      return applyAssistantDelta(state, event)
    case 'assistant.thinking.delta':
      return applyThinkingDelta(state, event)
    case 'assistant.completed':
      return applyAssistantCompleted(state, event)
    case 'tool.requested':
      if (!event.payload) {
        return state
      }
      return upsertToolItem(state, event, {
        id: event.payload.toolUseId,
        name: event.payload.toolName,
        status: 'queued',
        argumentsSummary: event.payload.argumentsSummary,
      })
    case 'tool.approved':
      if (!event.payload) {
        return state
      }
      return patchToolItem(state, event, event.payload.toolUseId, { status: 'running' })
    case 'tool.denied':
      if (!event.payload) {
        return state
      }
      return patchToolItem(state, event, event.payload.toolUseId, { status: 'denied' }, true)
    case 'tool.completed':
      if (!event.payload) {
        return state
      }
      return patchToolItem(state, event, event.payload.toolUseId, {
        status: 'completed',
        durationMs: event.payload.durationMs,
        outputSummary: event.payload.outputSummary,
      })
    case 'tool.failed':
      if (!event.payload) {
        return state
      }
      return patchToolItem(
        state,
        event,
        event.payload.toolUseId,
        { status: 'failed', errorMessage: event.payload.message ?? event.payload.code },
        true,
      )
    case 'permission.requested':
      if (!event.payload) {
        return state
      }
      return applyPermissionRequested(state, event)
    case 'permission.resolved':
      if (!event.payload) {
        return state
      }
      return patchPermissionBlock(state, event.payload.requestId, {
        status: 'resolved',
        decision: event.payload.decision,
        submitDecision: undefined,
      })
    case 'artifact.created':
    case 'artifact.updated':
      if (!event.payload) {
        return state
      }
      return upsertArtifactPlaceholder(state, event)
    case 'engine.failed':
      if (!event.payload) {
        return state
      }
      addBlock(state, {
        ...baseBlock(state, event, `error:${event.id}`),
        kind: 'error',
        message: event.payload.message,
      })
      state.activeRunIds = state.activeRunIds.filter((runId) => runId !== event.runId)
      removeThinkingBlocksForRun(state, event.runId)
      finalizeStreamingFromFailure(state, event.runId)
      return state
  }
}

function localSubmit(
  state: ConversationTimelineState,
  action: Extract<ConversationTimelineAction, { type: 'localSubmit' }>,
): ConversationTimelineState {
  const blockId = `local:${action.clientMessageId}`
  const block: UserMessageBlock = {
    id: blockId,
    kind: 'userMessage',
    conversationId: state.conversationId,
    clientMessageId: action.clientMessageId,
    conversationSequence: nextSequence(state),
    createdAt: action.at,
    body: action.draft.prompt,
    status: 'sending',
  }
  addBlock(state, block)
  state.optimisticBlocksByClientMessageId[action.clientMessageId] = blockId
  return state
}

function commandAccepted(
  state: ConversationTimelineState,
  clientMessageId: string,
  runId: string,
): ConversationTimelineState {
  state.clientMessageByRunId[runId] = clientMessageId
  state.activeRunIds = addUnique(state.activeRunIds, runId)
  return patchUserBlock(state, clientMessageId, { runId })
}

function applyUserMessageAppended(
  state: ConversationTimelineState,
  event: Extract<TimelineRunEvent, { type: 'user.message.appended' }>,
): ConversationTimelineState {
  const payload = event.payload
  if (!payload) {
    return state
  }

  const clientMessageId = payload.clientMessageId
  const blockId = clientMessageId
    ? state.optimisticBlocksByClientMessageId[clientMessageId]
    : undefined

  if (blockId && clientMessageId) {
    patchBlock(state, blockId, {
      id: `message:${payload.messageId}`,
      messageId: payload.messageId,
      runId: event.runId,
      body: payload.body,
      status: 'sent',
      updatedAt: event.timestamp,
    })
    delete state.optimisticBlocksByClientMessageId[clientMessageId]
    state.clientMessageByRunId[event.runId] = clientMessageId
    return state
  }

  if (
    selectBlocksFromState(state).some(
      (block) => 'messageId' in block && block.messageId === payload.messageId,
    )
  ) {
    return state
  }

  addBlock(state, {
    ...baseBlock(state, event, `message:${payload.messageId}`),
    kind: 'userMessage',
    messageId: payload.messageId,
    clientMessageId: payload.clientMessageId,
    body: payload.body,
    status: 'sent',
  })
  return state
}

function applyAssistantDelta(
  state: ConversationTimelineState,
  event: Extract<TimelineRunEvent, { type: 'assistant.delta' }>,
): ConversationTimelineState {
  const payload = event.payload
  if (!payload) {
    return state
  }

  return appendAssistantAnswerDelta(state, {
    runId: event.runId,
    text: payload.text,
    timestamp: event.timestamp,
    conversationSequence: event.conversationSequence,
    runSequence: event.sequence,
    conversationId: state.conversationId,
  })
}

function applyThinkingDelta(
  state: ConversationTimelineState,
  event: Extract<TimelineRunEvent, { type: 'assistant.thinking.delta' }>,
): ConversationTimelineState {
  const payload = event.payload
  if (!payload) {
    return state
  }

  return appendThinkingDelta(state, {
    runId: event.runId,
    text: payload.text,
    timestamp: event.timestamp,
    conversationSequence: event.conversationSequence,
    runSequence: event.sequence,
    conversationId: state.conversationId,
  })
}

function applyAssistantCompleted(
  state: ConversationTimelineState,
  event: Extract<TimelineRunEvent, { type: 'assistant.completed' }>,
): ConversationTimelineState {
  const payload = event.payload
  if (!payload) {
    return state
  }

  return finalizeAssistantMessage(state, {
    runId: event.runId,
    messageId: payload.messageId,
    body: payload.body,
    timestamp: event.timestamp,
    conversationSequence: event.conversationSequence,
    runSequence: event.sequence,
    conversationId: state.conversationId,
  })
}

function upsertToolItem(
  state: ConversationTimelineState,
  event: TimelineRunEvent,
  item: ToolItem,
): ConversationTimelineState {
  const group = ensureToolGroup(state, event)
  const itemIndex = group.items.findIndex((currentItem) => currentItem.id === item.id)
  const items =
    itemIndex >= 0
      ? group.items.map((currentItem) =>
          currentItem.id === item.id ? { ...currentItem, ...item } : currentItem,
        )
      : [...group.items, item]
  patchBlock(state, group.id, { items })
  return state
}

function patchToolItem(
  state: ConversationTimelineState,
  event: TimelineRunEvent,
  toolUseId: string,
  patch: Partial<ToolItem>,
  expanded = false,
): ConversationTimelineState {
  const group = ensureToolGroup(state, event)
  const fallbackItem: ToolItem = { id: toolUseId, name: toolUseId, status: 'queued', ...patch }
  const items = group.items.some((item) => item.id === toolUseId)
    ? group.items.map((item) => (item.id === toolUseId ? { ...item, ...patch } : item))
    : [...group.items, fallbackItem]
  patchBlock(state, group.id, { items, expanded: group.expanded || expanded })
  return state
}

function ensureToolGroup(
  state: ConversationTimelineState,
  event: TimelineRunEvent,
): ToolGroupBlock {
  const existingId = state.toolGroupBlockByRunId[event.runId]
  const existing = findBlockById(state, existingId)
  if (existing) {
    return existing as ToolGroupBlock
  }

  const block: ToolGroupBlock = {
    ...baseBlock(state, event, `tools:${event.runId}`),
    kind: 'toolGroup',
    items: [],
    expanded: false,
  }
  state.toolGroupBlockByRunId[event.runId] = block.id
  addBlock(state, block)
  return block
}

function applyPermissionRequested(
  state: ConversationTimelineState,
  event: Extract<TimelineRunEvent, { type: 'permission.requested' }>,
): ConversationTimelineState {
  const payload = event.payload
  if (!payload) {
    return state
  }
  const block: PermissionRequestBlock = {
    ...baseBlock(state, event, `permission:${payload.requestId}`),
    kind: 'permissionRequest',
    requestId: payload.requestId,
    operation: payload.operation,
    reason: payload.reason,
    target: payload.target,
    severity: payload.severity,
    decisionScope: payload.decisionScope,
    exposure: payload.exposure,
    workspaceBoundary: payload.workspaceBoundary,
    status: 'pending',
  }
  state.permissionBlockByRequestId[block.requestId] = block.id
  addBlock(state, block)
  return state
}

function upsertArtifactPlaceholder(
  state: ConversationTimelineState,
  event: Extract<TimelineRunEvent, { type: 'artifact.created' | 'artifact.updated' }>,
): ConversationTimelineState {
  const payload = event.payload
  if (!payload) {
    return state
  }
  const existingId = state.artifactBlockByArtifactId[payload.artifactId]
  if (existingId) {
    patchBlock(state, existingId, {
      status: payload.status,
      updatedAt: event.timestamp,
    })
    return state
  }

  const block: ArtifactBlock = {
    ...baseBlock(state, event, `artifact:${payload.artifactId}`),
    kind: 'artifact',
    artifactId: payload.artifactId,
    artifactKind: 'artifact',
    title: 'Artifact',
    description: '',
    actionLabel: 'Open',
    status: payload.status ?? 'pending',
  }
  state.artifactBlockByArtifactId[block.artifactId] = block.id
  addBlock(state, block)
  return state
}

function applyArtifacts(
  state: ConversationTimelineState,
  artifacts: ArtifactView[],
): ConversationTimelineState {
  for (const artifact of artifacts) {
    const existingId = state.artifactBlockByArtifactId[artifact.id]
    const patch = {
      title: artifact.title,
      description: artifact.description,
      actionLabel: artifact.actionLabel,
      artifactKind: artifact.kind,
      preview: artifact.preview,
      status: artifact.status,
    }

    if (existingId) {
      patchBlock(state, existingId, patch)
      continue
    }

    const block: ArtifactBlock = {
      id: `artifact:${artifact.id}`,
      kind: 'artifact',
      conversationId: state.conversationId,
      conversationSequence: nextSequence(state),
      createdAt: new Date(0).toISOString(),
      artifactId: artifact.id,
      ...patch,
    }
    state.artifactBlockByArtifactId[artifact.id] = block.id
    addBlock(state, block)
  }
  return state
}

function markAssistantReconcile(
  state: ConversationTimelineState,
  runId: string,
  messageId: string,
): ConversationTimelineState {
  state.pendingAssistantReconcileByMessageId[messageId] = true
  const blockId = state.streamingBlockByRunId[runId]
  if (blockId) {
    patchBlock(state, blockId, { status: 'interrupted' })
  }
  return state
}

function snapshotReconciled(
  state: ConversationTimelineState,
  snapshot: ConversationSnapshot,
): ConversationTimelineState {
  for (const message of snapshot.messages) {
    if (!state.pendingAssistantReconcileByMessageId[message.id] || message.author !== 'assistant') {
      continue
    }
    finalizeAssistantMessage(state, {
      runId: '',
      messageId: message.id,
      body: message.body,
      timestamp: message.timestamp,
      conversationSequence: nextSequence(state),
      conversationId: snapshot.id,
    })
  }
  return state
}

function finalizeStreamingFromRunEnd(state: ConversationTimelineState, runId: string) {
  const blockId = state.streamingBlockByRunId[runId]
  if (blockId) {
    patchBlock(state, blockId, { status: 'interrupted' })
  }
}

function finalizeStreamingFromFailure(state: ConversationTimelineState, runId: string) {
  finalizeStreamingFromRunEnd(state, runId)
}

function patchUserBlock(
  state: ConversationTimelineState,
  clientMessageId: string,
  patch: Partial<UserMessageBlock>,
): ConversationTimelineState {
  const blockId = state.optimisticBlocksByClientMessageId[clientMessageId]
  if (!blockId) {
    return state
  }
  patchBlock(state, blockId, patch)
  return state
}

function patchPermissionBlock(
  state: ConversationTimelineState,
  requestId: string,
  patch: Partial<PermissionRequestBlock>,
): ConversationTimelineState {
  const blockId = state.permissionBlockByRequestId[requestId]
  if (!blockId) {
    return state
  }
  patchBlock(state, blockId, patch)
  return state
}

function baseBlock(
  state: ConversationTimelineState,
  event: TimelineRunEvent,
  id: string,
): Omit<ConversationBlock, 'kind'> {
  return {
    id,
    conversationId: state.conversationId,
    runId: event.runId,
    conversationSequence: event.conversationSequence,
    runSequence: event.sequence,
    createdAt: event.timestamp,
  }
}

function addUnique(values: string[], value: string) {
  return values.includes(value) ? values : [...values, value]
}
