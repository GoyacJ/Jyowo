import type { ComposerSubmitPayload } from '../Composer'
import type { ConversationTimelineState } from './conversation-timeline-store'

export type ConversationTimelineAction =
  | {
      type: 'hydrateWorktree'
      page: Pick<
        ConversationTimelineState,
        'turns' | 'pageCursor' | 'eventCursor' | 'hasMoreBefore' | 'hasMoreAfter' | 'gap'
      >
    }
  | { type: 'localSubmit'; clientMessageId: string; draft: ComposerSubmitPayload; at: string }
  | { type: 'commandAccepted'; clientMessageId: string; runId: string }
  | { type: 'commandFailed'; clientMessageId: string; errorMessage: string }
  | { type: 'permissionSubmitting'; requestId: string; decision: 'approve' | 'deny' }
  | { type: 'permissionSubmitFailed'; requestId: string; errorMessage: string }
  | { type: 'markGap' }
  | { type: 'worktreeRefreshRequested'; immediate: boolean }
