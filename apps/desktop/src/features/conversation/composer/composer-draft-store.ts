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

// In-memory draft per conversation or task. Never persisted to localStorage
// to avoid accidentally storing secrets, API keys, or private paths.
const draftsByScope = new Map<string, ComposerDraft>()

export function getDraft(scopeId: string): ComposerDraft {
  return draftsByScope.get(scopeId) ?? { ...emptyDraft }
}

export function saveDraft(scopeId: string, draft: ComposerDraft): void {
  draftsByScope.set(scopeId, { ...draft })
}

export function clearDraft(scopeId: string): void {
  draftsByScope.delete(scopeId)
}

export function getEmptyDraft(): ComposerDraft {
  return { ...emptyDraft }
}
