import type { ThinkingBlock } from './conversation-blocks'
import { addBlock, findBlockById, patchBlock, removeBlockById } from './conversation-timeline-index'
import type { ConversationTimelineState } from './conversation-timeline-reducer'

export function appendThinkingDelta(
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
  const existingId = state.thinkingBlockByRunId[input.runId]
  if (existingId) {
    const currentBlock = findBlockById(state, existingId)
    const block = currentBlock?.kind === 'thinking' ? (currentBlock as ThinkingBlock) : undefined
    if (block) {
      patchBlock(state, block.id, {
        body: `${block.body}${input.text}`,
        status: 'streaming',
        updatedAt: input.timestamp,
      })
      return state
    }
  }

  const block: ThinkingBlock = {
    id: `thinking:${input.runId}`,
    kind: 'thinking',
    conversationId: input.conversationId,
    runId: input.runId,
    conversationSequence: input.conversationSequence,
    runSequence: input.runSequence,
    createdAt: input.timestamp,
    body: input.text,
    status: 'streaming',
    collapsed: true,
  }
  state.thinkingBlockByRunId[input.runId] = block.id
  addBlock(state, block)
  return state
}

export function removeThinkingBlocksForRun(
  state: ConversationTimelineState,
  runId: string,
): ConversationTimelineState {
  const blockId = state.thinkingBlockByRunId[runId]
  if (blockId) {
    removeBlockById(state, blockId)
    delete state.thinkingBlockByRunId[runId]
  }
  return state
}
