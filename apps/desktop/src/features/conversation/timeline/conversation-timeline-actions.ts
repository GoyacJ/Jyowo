import type {
  ConversationCursor,
  ConversationTurn,
  ConversationTurnCursor,
} from '@/shared/tauri/commands'
import type { ComposerSubmitPayload } from '../Composer'

export type ConversationTimelineAction =
  | {
      type: 'hydrateInitialPage'
      turns: ConversationTurn[]
      pageCursor: ConversationTurnCursor | null
      eventCursor: ConversationCursor | null
      hasMoreBefore: boolean
      hasMoreAfter: boolean
      gap: boolean
    }
  | {
      type: 'prependPage'
      turns: ConversationTurn[]
      pageCursor: ConversationTurnCursor | null
      hasMoreBefore: boolean
    }
  | {
      type: 'appendPage'
      turns: ConversationTurn[]
      pageCursor: ConversationTurnCursor | null
      eventCursor: ConversationCursor | null
      hasMoreAfter: boolean
    }
  | { type: 'localSubmit'; clientMessageId: string; draft: ComposerSubmitPayload; at: string }
  | { type: 'commandAccepted'; clientMessageId: string; runId: string }
  | { type: 'commandFailed'; clientMessageId: string; errorMessage: string }
  | { type: 'permissionSubmitting'; requestId: string; decision: 'approve' | 'deny' }
  | { type: 'permissionSubmitFailed'; requestId: string; errorMessage: string }
  | { type: 'markGap'; afterCursor: ConversationCursor | null }
  | { type: 'retryGap' }
  | { type: 'worktreeRefreshRequested'; immediate: boolean }
