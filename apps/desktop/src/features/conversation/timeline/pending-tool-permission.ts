import type { DecisionRequestState, ToolAttempt } from '@/shared/tauri/commands'

export type PendingToolPermission = DecisionRequestState & {
  conversationId: string
  toolAttempt: ToolAttempt
  turnId: string
}
