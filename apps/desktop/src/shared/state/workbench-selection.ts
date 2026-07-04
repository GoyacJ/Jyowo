import type { ConversationEventRef } from '@/shared/tauri/commands'

type EvidenceRefId = string

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
