import type { AssistantSegment, ConversationTurn, ToolAttempt } from '@/shared/tauri/commands'
import type { ConversationTimelineState } from './conversation-timeline-store'
import type { PendingToolPermission } from './pending-tool-permission'

export type ComposerMode =
  | { kind: 'ready' }
  | { kind: 'submitting' }
  | { kind: 'running-disabled'; canCancel: boolean }
  | { kind: 'clarification-reply'; segmentId: string }
  | { kind: 'review-comment'; segmentId: string }
  | { kind: 'retry'; turnId: string }
  | { kind: 'continue' }

export function selectTurns(state: ConversationTimelineState): ConversationTurn[] {
  return state.turns
}

export function selectComposerMode(state: ConversationTimelineState): ComposerMode {
  if (state.turns.some((turn) => turn.assistant?.status === 'running')) {
    return { kind: 'running-disabled', canCancel: true }
  }

  const pendingClarification = findLastSegment(
    state.turns,
    (segment) => segment.kind === 'clarificationRequest',
  )
  if (pendingClarification?.kind === 'clarificationRequest') {
    return { kind: 'clarification-reply', segmentId: pendingClarification.id }
  }

  const pendingReview = findLastSegment(state.turns, (segment) => segment.kind === 'reviewRequest')
  if (pendingReview?.kind === 'reviewRequest') {
    return { kind: 'review-comment', segmentId: pendingReview.id }
  }

  const failedTurn = [...state.turns].reverse().find((turn) => turn.assistant?.status === 'failed')
  if (failedTurn) {
    return { kind: 'retry', turnId: failedTurn.id }
  }

  return { kind: 'ready' }
}

export function selectPendingPermissions(
  state: ConversationTimelineState,
): PendingToolPermission[] {
  const pending: PendingToolPermission[] = []

  for (const turn of state.turns) {
    for (const attempt of toolAttempts(turn)) {
      const permission = attempt.permission
      if (!permission || (permission.status !== 'pending' && permission.status !== 'submitting')) {
        continue
      }
      pending.push({
        ...permission,
        conversationId: turn.conversationId,
        toolAttempt: attempt,
        turnId: turn.id,
      })
    }
  }

  return pending
}

export function selectShouldPollFallback(state: ConversationTimelineState): boolean {
  return state.gap
}

export type TurnGroup = {
  turnId: string
  turns: ConversationTurn[]
}

export function selectTurnGroups(turns: ConversationTurn[]): TurnGroup[] {
  return turns.map((turn) => ({ turnId: turn.id, turns: [turn] }))
}

function findLastSegment(
  turns: ConversationTurn[],
  predicate: (segment: AssistantSegment) => boolean,
) {
  for (const turn of [...turns].reverse()) {
    const segment = [...(turn.assistant?.segments ?? [])].reverse().find(predicate)
    if (segment) {
      return segment
    }
  }
  return undefined
}

function toolAttempts(turn: ConversationTurn): ToolAttempt[] {
  return (turn.assistant?.segments ?? []).flatMap((segment) =>
    segment.kind === 'toolGroup' ? segment.attempts : [],
  )
}
