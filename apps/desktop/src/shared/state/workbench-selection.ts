import type { ConversationEventRef } from '@/shared/tauri/commands'

type EvidenceRefId = string

export type TaskWorkbenchPanel =
  | 'changes'
  | 'commands'
  | 'agents'
  | 'environment'
  | 'sources'
  | 'audit'

export type TaskWorkbenchMode = 'closed' | 'inspector' | 'collaboration'

export type TaskWorkbenchSelection = {
  blobId?: string
  eventId?: string
  panel: TaskWorkbenchPanel
  segmentId?: string
  taskId: string
}

export type WorkbenchSelection =
  | { kind: 'context' }
  | { kind: 'decision'; conversationId: string; requestId: string }
  | { kind: 'tool'; conversationId: string; toolUseId: string }
  | {
      kind: 'command'
      conversationId: string
      fullOutputRef?: EvidenceRefId
      eventRef?: ConversationEventRef
    }
  | { kind: 'diff'; conversationId: string; changeSetId: string }
  | {
      kind: 'artifact'
      conversationId: string
      artifactId: string
      revisionId?: string
    }
