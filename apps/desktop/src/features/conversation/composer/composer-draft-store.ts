import type { AttachmentReference, ContextReference } from '@/shared/tauri/commands'

export type ComposerDraft = {
  attachments: AttachmentReference[]
  contextReferences: ContextReference[]
  text: string
}

const emptyDraft: ComposerDraft = {
  attachments: [],
  contextReferences: [],
  text: '',
}

// In-memory draft per conversation. Never persisted to localStorage
// to avoid accidentally storing secrets, API keys, or private paths.
const draftsByConversation = new Map<string, ComposerDraft>()

export function getDraft(conversationId: string): ComposerDraft {
  return draftsByConversation.get(conversationId) ?? { ...emptyDraft }
}

export function saveDraft(conversationId: string, draft: ComposerDraft): void {
  draftsByConversation.set(conversationId, { ...draft })
}

export function clearDraft(conversationId: string): void {
  draftsByConversation.delete(conversationId)
}

export function getEmptyDraft(): ComposerDraft {
  return { ...emptyDraft }
}
