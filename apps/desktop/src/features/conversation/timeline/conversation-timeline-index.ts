import type {
  AssistantMessageBlock,
  AssistantStreamingBlock,
  ConversationBlock,
  ConversationSnapshot,
  UserMessageBlock,
} from './conversation-blocks'
import type { ConversationTimelineState } from './conversation-timeline-reducer'

function messageBlockId(messageId: string) {
  return `message:${messageId}`
}

function streamingBlockId(runId: string) {
  return `assistant-stream:${runId}`
}

function findBlockByMessageId(
  state: ConversationTimelineState,
  messageId: string,
): ConversationBlock | undefined {
  const indexedId = state.assistantBlockByMessageId[messageId]
  if (indexedId) {
    return state.blocksById[indexedId]
  }

  const canonicalId = messageBlockId(messageId)
  return state.blocksById[canonicalId]
}

function hasFinalAssistantMessage(state: ConversationTimelineState, messageId: string) {
  const block = findBlockByMessageId(state, messageId)
  return block?.kind === 'assistantMessage' && block.status === 'complete'
}

function removeBlock(state: ConversationTimelineState, blockId: string) {
  const block = state.blocksById[blockId]
  if (block) {
    unindexBlock(state, block)
  }
  state.blockOrder = state.blockOrder.filter((id) => id !== blockId)
  delete state.blocksById[blockId]
}

export function sortBlocks(state: ConversationTimelineState) {
  state.blockOrder.sort(
    (left, right) =>
      state.blocksById[left].conversationSequence - state.blocksById[right].conversationSequence,
  )
}

export function nextSequence(state: ConversationTimelineState) {
  return (
    selectBlocksFromState(state).reduce(
      (max, block) => Math.max(max, block.conversationSequence),
      -1,
    ) + 1
  )
}

export function addBlock(state: ConversationTimelineState, block: ConversationBlock) {
  if (state.blocksById[block.id]) {
    patchBlock(state, block.id, block)
    return
  }
  state.blocksById[block.id] = block
  state.blockOrder.push(block.id)
  sortBlocks(state)
  indexBlock(state, block)
}

export function patchBlock(
  state: ConversationTimelineState,
  blockId: string,
  patch: Partial<ConversationBlock> & { id?: string },
) {
  const block = state.blocksById[blockId]
  if (!block) {
    return
  }
  const next = { ...block, ...patch } as ConversationBlock
  unindexBlock(state, block)
  if (next.id !== blockId) {
    delete state.blocksById[blockId]
    state.blockOrder = state.blockOrder.map((id) => (id === blockId ? next.id : id))
  }
  state.blocksById[next.id] = next
  indexBlock(state, next)
  sortBlocks(state)
}

function indexBlocks(state: ConversationTimelineState) {
  state.optimisticBlocksByClientMessageId = {}
  state.artifactBlockByArtifactId = {}
  state.permissionBlockByRequestId = {}
  state.streamingBlockByRunId = {}
  state.toolGroupBlockByRunId = {}
  state.assistantBlockByMessageId = {}

  state.thinkingBlockByRunId = {}
  for (const block of selectBlocksFromState(state)) {
    indexBlock(state, block)
  }
}

function indexBlock(state: ConversationTimelineState, block: ConversationBlock) {
  if (block.kind === 'userMessage' && block.clientMessageId && !block.messageId) {
    state.optimisticBlocksByClientMessageId[block.clientMessageId] = block.id
  }
  if (block.kind === 'artifact') {
    state.artifactBlockByArtifactId[block.artifactId] = block.id
  }
  if (block.kind === 'permissionRequest') {
    state.permissionBlockByRequestId[block.requestId] = block.id
  }
  if (block.kind === 'assistantStreaming' && block.runId) {
    state.streamingBlockByRunId[block.runId] = block.id
  }
  if (block.kind === 'toolGroup' && block.runId) {
    state.toolGroupBlockByRunId[block.runId] = block.id
  }
  if (block.kind === 'thinking' && block.runId) {
    state.thinkingBlockByRunId[block.runId] = block.id
  }
  if (
    (block.kind === 'assistantMessage' || block.kind === 'assistantStreaming') &&
    block.messageId
  ) {
    state.assistantBlockByMessageId[block.messageId] = block.id
  }
}

function unindexBlock(state: ConversationTimelineState, block: ConversationBlock) {
  if (block.kind === 'userMessage' && block.clientMessageId) {
    deleteIfIndexed(state.optimisticBlocksByClientMessageId, block.clientMessageId, block.id)
  }
  if (block.kind === 'artifact') {
    deleteIfIndexed(state.artifactBlockByArtifactId, block.artifactId, block.id)
  }
  if (block.kind === 'permissionRequest') {
    deleteIfIndexed(state.permissionBlockByRequestId, block.requestId, block.id)
  }
  if (block.kind === 'assistantStreaming' && block.runId) {
    deleteIfIndexed(state.streamingBlockByRunId, block.runId, block.id)
  }
  if (block.kind === 'toolGroup' && block.runId) {
    deleteIfIndexed(state.toolGroupBlockByRunId, block.runId, block.id)
  }
  if (block.kind === 'thinking' && block.runId) {
    deleteIfIndexed(state.thinkingBlockByRunId, block.runId, block.id)
  }
  if (
    (block.kind === 'assistantMessage' || block.kind === 'assistantStreaming') &&
    block.messageId
  ) {
    deleteIfIndexed(state.assistantBlockByMessageId, block.messageId, block.id)
  }
}

function deleteIfIndexed(index: Record<string, string>, key: string, blockId: string) {
  if (index[key] === blockId) {
    delete index[key]
  }
}

export function selectBlocksFromState(state: ConversationTimelineState): ConversationBlock[] {
  return state.blockOrder.map((id) => state.blocksById[id]).filter(Boolean)
}

export function findBlockById(
  state: ConversationTimelineState,
  blockId: string | undefined,
): ConversationBlock | undefined {
  return blockId ? state.blocksById[blockId] : undefined
}

export function removeBlockById(state: ConversationTimelineState, blockId: string) {
  removeBlock(state, blockId)
}

export function reconcileSnapshotMessages(
  state: ConversationTimelineState,
  snapshot: ConversationSnapshot,
): ConversationTimelineState {
  if (state.conversationId !== snapshot.id) {
    return reconcileSnapshotMessages(createFreshStateForSnapshot(snapshot), snapshot)
  }

  const snapshotMessageIds = new Set(snapshot.messages.map((message) => messageBlockId(message.id)))
  const nonSnapshotSequenceById = new Map(
    selectBlocksFromState(state)
      .filter((block) => !snapshotMessageIds.has(block.id))
      .map((block, index) => [block.id, snapshot.messages.length + index]),
  )

  state.blocksById = Object.fromEntries(
    selectBlocksFromState(state).map((block) => {
      const next = snapshotMessageIds.has(block.id)
        ? block
        : ({
            ...block,
            conversationId: snapshot.id,
            conversationSequence:
              nonSnapshotSequenceById.get(block.id) ?? block.conversationSequence,
          } as ConversationBlock)
      return [next.id, next]
    }),
  )
  state.blockOrder = selectBlocksFromState(state)
    .sort((left, right) => left.conversationSequence - right.conversationSequence)
    .map((block) => block.id)

  for (const [index, message] of snapshot.messages.entries()) {
    upsertSnapshotMessage(state, snapshot.id, message, index)
  }

  indexBlocks(state)
  return state
}

function createFreshStateForSnapshot(snapshot: ConversationSnapshot): ConversationTimelineState {
  return {
    conversationId: snapshot.id,
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

function upsertSnapshotMessage(
  state: ConversationTimelineState,
  conversationId: string,
  message: ConversationSnapshot['messages'][number],
  index: number,
) {
  const base = {
    conversationId,
    conversationSequence: index,
    createdAt: message.timestamp,
    id: messageBlockId(message.id),
    messageId: message.id,
  }

  if (message.author === 'user') {
    upsertSnapshotUserMessage(state, message, base)
    return
  }

  if (message.author !== 'assistant') {
    return
  }

  if (state.pendingAssistantReconcileByMessageId[message.id]) {
    return
  }

  upsertSnapshotAssistantMessage(state, message, base)
}

function upsertSnapshotUserMessage(
  state: ConversationTimelineState,
  message: ConversationSnapshot['messages'][number],
  base: {
    conversationId: string
    conversationSequence: number
    createdAt: string
    id: string
    messageId: string
  },
) {
  const optimisticBlock = findOptimisticUserBlockForSnapshot(state, message.clientMessageId)
  if (optimisticBlock) {
    patchBlock(state, optimisticBlock.id, {
      ...base,
      kind: 'userMessage',
      body: message.body,
      clientMessageId: message.clientMessageId,
      status: 'sent',
      updatedAt: message.timestamp,
    } satisfies UserMessageBlock)
    if (message.clientMessageId) {
      delete state.optimisticBlocksByClientMessageId[message.clientMessageId]
    }
    return
  }

  const existing = state.blocksById[base.id]
  if (existing) {
    patchBlock(state, base.id, {
      kind: 'userMessage',
      body: message.body,
      clientMessageId: message.clientMessageId,
      status: 'sent',
      updatedAt: message.timestamp,
    } satisfies Partial<UserMessageBlock>)
    return
  }

  addBlock(state, {
    ...base,
    kind: 'userMessage',
    body: message.body,
    clientMessageId: message.clientMessageId,
    status: 'sent',
  } satisfies UserMessageBlock)
}

function upsertSnapshotAssistantMessage(
  state: ConversationTimelineState,
  message: {
    body: string
    id: string
    timestamp: string
  },
  base: {
    conversationId: string
    conversationSequence: number
    createdAt: string
    id: string
    messageId: string
  },
) {
  const existing = findBlockByMessageId(state, message.id)
  if (existing) {
    patchBlock(state, existing.id, {
      id: base.id,
      kind: 'assistantMessage',
      body: message.body,
      status: 'complete',
      updatedAt: message.timestamp,
    } satisfies Partial<AssistantMessageBlock>)
    const streamingId = Object.entries(state.streamingBlockByRunId).find(
      ([, blockId]) => blockId !== base.id && blockId === existing.id,
    )?.[1]
    if (streamingId && streamingId !== base.id) {
      removeBlock(state, streamingId)
    }
    return
  }

  const duplicateStreaming = selectBlocksFromState(state).find(
    (block): block is AssistantStreamingBlock =>
      block.kind === 'assistantStreaming' && block.messageId === message.id,
  )
  if (duplicateStreaming) {
    patchBlock(state, duplicateStreaming.id, {
      ...base,
      kind: 'assistantMessage',
      body: message.body,
      status: 'complete',
      updatedAt: message.timestamp,
    } satisfies Partial<AssistantMessageBlock>)
    if (duplicateStreaming.runId) {
      delete state.streamingBlockByRunId[duplicateStreaming.runId]
    }
    return
  }

  addBlock(state, {
    ...base,
    kind: 'assistantMessage',
    body: message.body,
    status: 'complete',
  } satisfies AssistantMessageBlock)
}

function findOptimisticUserBlockForSnapshot(
  state: ConversationTimelineState,
  clientMessageId: string | undefined,
): UserMessageBlock | undefined {
  if (!clientMessageId) {
    return undefined
  }
  const indexedId = state.optimisticBlocksByClientMessageId[clientMessageId]
  if (indexedId) {
    const indexed = state.blocksById[indexedId]
    if (indexed?.kind === 'userMessage' && !indexed.messageId) {
      return indexed
    }
  }
  return selectBlocksFromState(state).find(
    (block): block is UserMessageBlock =>
      block.kind === 'userMessage' && block.clientMessageId === clientMessageId && !block.messageId,
  )
}

export function finalizeAssistantMessage(
  state: ConversationTimelineState,
  input: {
    runId: string
    messageId: string
    body?: string
    timestamp: string
    conversationSequence: number
    runSequence?: number
    conversationId: string
  },
): ConversationTimelineState {
  const finalBlockId = messageBlockId(input.messageId)

  if (!input.body) {
    state.pendingAssistantReconcileByMessageId[input.messageId] = true
    const streamingId = state.streamingBlockByRunId[input.runId]
    if (streamingId) {
      patchBlock(state, streamingId, {
        messageId: input.messageId,
        updatedAt: input.timestamp,
      })
    }
    return state
  }

  const existingFinal = findBlockByMessageId(state, input.messageId)
  const streamingId = state.streamingBlockByRunId[input.runId]

  if (existingFinal) {
    patchBlock(state, existingFinal.id, {
      id: finalBlockId,
      kind: 'assistantMessage',
      messageId: input.messageId,
      body: input.body,
      status: 'complete',
      updatedAt: input.timestamp,
      runId: input.runId,
    } satisfies Partial<AssistantMessageBlock>)
    if (streamingId && streamingId !== finalBlockId && streamingId !== existingFinal.id) {
      removeBlock(state, streamingId)
    }
    delete state.streamingBlockByRunId[input.runId]
    delete state.pendingAssistantReconcileByMessageId[input.messageId]
    return state
  }

  if (streamingId) {
    patchBlock(state, streamingId, {
      id: finalBlockId,
      kind: 'assistantMessage',
      messageId: input.messageId,
      body: input.body,
      status: 'complete',
      updatedAt: input.timestamp,
    } satisfies Partial<AssistantMessageBlock>)
    delete state.streamingBlockByRunId[input.runId]
    delete state.pendingAssistantReconcileByMessageId[input.messageId]
    return state
  }

  addBlock(state, {
    id: finalBlockId,
    kind: 'assistantMessage',
    conversationId: input.conversationId,
    runId: input.runId,
    conversationSequence: input.conversationSequence,
    runSequence: input.runSequence,
    createdAt: input.timestamp,
    messageId: input.messageId,
    body: input.body,
    status: 'complete',
  } satisfies AssistantMessageBlock)
  delete state.pendingAssistantReconcileByMessageId[input.messageId]
  return state
}

export function appendAssistantAnswerDelta(
  state: ConversationTimelineState,
  input: {
    runId: string
    text: string
    timestamp: string
    conversationSequence: number
    runSequence?: number
    conversationId: string
  },
): ConversationTimelineState {
  const streamingId = state.streamingBlockByRunId[input.runId]
  if (streamingId) {
    const streamingBlock = selectBlocksFromState(state).find(
      (block): block is AssistantStreamingBlock =>
        block.id === streamingId && block.kind === 'assistantStreaming',
    )
    if (streamingBlock?.messageId && hasFinalAssistantMessage(state, streamingBlock.messageId)) {
      return state
    }
    if (streamingBlock) {
      patchBlock(state, streamingBlock.id, {
        body: `${streamingBlock.body}${input.text}`,
        updatedAt: input.timestamp,
      })
      return state
    }
  }

  const blockId = streamingBlockId(input.runId)
  state.streamingBlockByRunId[input.runId] = blockId
  addBlock(state, {
    id: blockId,
    kind: 'assistantStreaming',
    conversationId: input.conversationId,
    runId: input.runId,
    conversationSequence: input.conversationSequence,
    runSequence: input.runSequence,
    createdAt: input.timestamp,
    body: input.text,
    status: 'streaming',
  } satisfies AssistantStreamingBlock)
  return state
}
