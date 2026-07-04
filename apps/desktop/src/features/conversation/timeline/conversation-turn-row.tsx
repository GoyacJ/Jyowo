import { memo } from 'react'

import type {
  ConversationEventRef,
  ConversationTurn,
  ResolvePermissionRequest,
} from '@/shared/tauri/commands'
import { ConversationTurnView } from './conversation-turn-view'

export const ConversationTurnRow = memo(function ConversationTurnRow({
  turn,
  onOpenDetails,
  onPermissionResolve,
  onReviewContinue,
}: {
  turn: ConversationTurn
  onOpenDetails?: (eventRef: ConversationEventRef) => void
  onPermissionResolve?: (request: ResolvePermissionRequest) => void
  onReviewContinue?: (prompt: string) => void
}) {
  return (
    <div data-conversation-turn-id={turn.id} id={`conversation-turn-${turn.id}`}>
      <ConversationTurnView
        onOpenDetails={onOpenDetails}
        onPermissionResolve={onPermissionResolve}
        onReviewContinue={onReviewContinue}
        turn={turn}
      />
    </div>
  )
})
