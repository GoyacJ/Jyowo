import type { ConversationBlock, PermissionRequestBlock } from './conversation-blocks'
import type { ConversationTimelineState } from './conversation-timeline-reducer'

export type ComposerMode =
  | { kind: 'ready' }
  | { kind: 'submitting' }
  | { kind: 'running-disabled'; canCancel: boolean }
  | { kind: 'clarification-reply'; blockId: string }
  | { kind: 'review-comment'; blockId: string }
  | { kind: 'retry'; turnId: string }
  | { kind: 'continue' }

export function selectBlocks(state: ConversationTimelineState): ConversationBlock[] {
  return state.blocks
}

export function selectComposerMode(state: ConversationTimelineState): ComposerMode {
  if (state.activeRunIds.length > 0) {
    return { kind: 'running-disabled', canCancel: true }
  }

  const clarification = state.blocks.find(
    (block) => block.kind === 'clarificationRequest' && block.status === 'pending',
  )
  if (clarification) {
    return { kind: 'clarification-reply', blockId: clarification.id }
  }

  const review = state.blocks.find(
    (block) => block.kind === 'reviewRequest' && block.status === 'pending',
  )
  if (review) {
    return { kind: 'review-comment', blockId: review.id }
  }

  const failedTurn = [...state.blocks].reverse().find((block) => block.status === 'failed')
  if (failedTurn?.turnId) {
    return { kind: 'retry', turnId: failedTurn.turnId }
  }

  return { kind: 'ready' }
}

export function selectPendingPermissionBlocks(
  state: ConversationTimelineState,
): PermissionRequestBlock[] {
  return state.blocks.filter(
    (block): block is PermissionRequestBlock =>
      block.kind === 'permissionRequest' &&
      (block.status === 'pending' || block.status === 'submitting'),
  )
}

export function selectShouldPollFallback(state: ConversationTimelineState): boolean {
  return state.hasGap
}

export type TurnGroup = {
  turnId: string
  blocks: ConversationBlock[]
}

export function selectTurnGroups(blocks: ConversationBlock[]): TurnGroup[] {
  const groups: TurnGroup[] = []
  let current: TurnGroup | null = null

  for (const block of blocks) {
    const turnId = block.turnId ?? `orphan:${block.id}`
    if (!current || current.turnId !== turnId) {
      current = { turnId, blocks: [block] }
      groups.push(current)
      continue
    }

    current.blocks.push(block)
  }

  return groups
}
