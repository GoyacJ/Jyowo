import type { ConversationCursor } from '@/shared/tauri/commands'
import type { ComposerSubmitPayload } from '../Composer'
import type { ArtifactView, ConversationSnapshot, TimelineRunEvent } from './conversation-blocks'

export type ConversationTimelineAction =
  | { type: 'hydrateSnapshot'; snapshot: ConversationSnapshot }
  | { type: 'applyEvents'; events: TimelineRunEvent[]; cursor?: ConversationCursor | null }
  | { type: 'applyArtifacts'; artifacts: ArtifactView[] }
  | { type: 'localSubmit'; clientMessageId: string; draft: ComposerSubmitPayload; at: string }
  | { type: 'commandAccepted'; clientMessageId: string; runId: string }
  | { type: 'commandFailed'; clientMessageId: string; errorMessage: string }
  | { type: 'assistantFinalContentMissing'; runId: string; messageId: string }
  | { type: 'snapshotReconciled'; snapshot: ConversationSnapshot }
  | { type: 'permissionSubmitting'; requestId: string; decision: 'approve' | 'deny' }
  | { type: 'permissionSubmitFailed'; requestId: string; errorMessage: string }
  | { type: 'markGap'; runId?: string; afterCursor?: ConversationCursor }
