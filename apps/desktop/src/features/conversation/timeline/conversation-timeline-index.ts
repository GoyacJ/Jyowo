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
    return state.blocks.find((block) => block.id === indexedId)
  }

  const canonicalId = messageBlockId(messageId)
  return state.blocks.find((block) => block.id === canonicalId)
}

function hasFinalAssistantMessage(state: ConversationTimelineState, messageId: string) {
  const block = findBlockByMessageId(state, messageId)
  return block?.kind === 'assistantMessage' && block.status === 'complete'
}

function removeBlock(state: ConversationTimelineState, blockId: string) {
  state.blocks = state.blocks.filter((block) => block.id !== blockId)
  indexBlocks(state)
}

export function sortBlocks(state: ConversationTimelineState) {
  state.blocks.sort((left, right) => left.conversationSequence - right.conversationSequence)
}

export function nextSequence(state: ConversationTimelineState) {
  return state.blocks.reduce((max, block) => Math.max(max, block.conversationSequence), -1) + 1
}

export function addBlock(state: ConversationTimelineState, block: ConversationBlock) {
  if (state.blocks.some((currentBlock) => currentBlock.id === block.id)) {
    patchBlock(state, block.id, block)
    return
  }
  state.blocks.push(block)
  sortBlocks(state)
}

export function patchBlock(
  state: ConversationTimelineState,
  blockId: string,
  patch: Partial<ConversationBlock> & { id?: string },
) {
  state.blocks = state.blocks.map((block) =>
    block.id === blockId ? ({ ...block, ...patch } as ConversationBlock) : block,
  )
  indexBlocks(state)
}

export function indexBlocks(state: ConversationTimelineState) {
  state.optimisticBlocksByClientMessageId = {}
  state.artifactBlockByArtifactId = {}
  state.permissionBlockByRequestId = {}
  state.streamingBlockByRunId = {}
  state.toolGroupBlockByRunId = {}
  state.assistantBlockByMessageId = {}

  state.thinkingBlockByRunId = {}
  for (const block of state.blocks) {
    if (block.kind === 'userMessage' && block.clientMessageId && block.status !== 'sent') {
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
    state.blocks
      .filter((block) => !snapshotMessageIds.has(block.id))
      .map((block, index) => [block.id, snapshot.messages.length + index]),
  )

  state.blocks = state.blocks
    .map((block) =>
      snapshotMessageIds.has(block.id)
        ? block
        : {
            ...block,
            conversationId: snapshot.id,
            conversationSequence:
              nonSnapshotSequenceById.get(block.id) ?? block.conversationSequence,
          },
    )
    .sort((left, right) => left.conversationSequence - right.conversationSequence)

  for (const [index, message] of snapshot.messages.entries()) {
    upsertSnapshotMessage(state, snapshot.id, message, index)
  }

  indexBlocks(state)
  return state
}

function createFreshStateForSnapshot(snapshot: ConversationSnapshot): ConversationTimelineState {
  return {
    conversationId: snapshot.id,
    blocks: [],
    eventsById: {},
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

  const existing = state.blocks.find((block) => block.id === base.id)
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

  const duplicateStreaming = state.blocks.find(
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
  const blockId = state.optimisticBlocksByClientMessageId[clientMessageId]
  return state.blocks.find(
    (block): block is UserMessageBlock =>
      block.id === blockId && block.kind === 'userMessage' && !block.messageId,
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
    const streamingBlock = state.blocks.find(
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
