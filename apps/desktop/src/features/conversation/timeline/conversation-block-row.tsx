import { memo } from 'react'

import type { ConversationEventRef, ConversationTurn } from '@/shared/tauri/commands'
import { ConversationTurnView } from './conversation-turn-view'

export const ConversationBlockRow = memo(function ConversationBlockRow({
  turn,
  onOpenDetails,
  onPermissionResolve,
  onReviewContinue,
}: {
  turn: ConversationTurn
  onOpenDetails?: (eventRef: ConversationEventRef) => void
  onPermissionResolve?: (request: {
    conversationId: string
    requestId: string
    decision: 'approve' | 'deny'
  }) => void
  onReviewContinue?: (prompt: string) => void
}) {
  return (
    <div data-conversation-turn-id={turn.id} id={`conversation-block-${turn.id}`}>
      <ConversationTurnView
        onOpenDetails={onOpenDetails}
        onPermissionResolve={onPermissionResolve}
        onReviewContinue={onReviewContinue}
        turn={turn}
      />
    </div>
  )
})
