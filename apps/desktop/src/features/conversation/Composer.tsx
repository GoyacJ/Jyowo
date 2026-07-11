import { Send, X } from 'lucide-react'
import type { KeyboardEvent } from 'react'
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type {
  AttachmentInputModality,
  AttachmentReference,
  ContextReference,
  ConversationModelCapability,
  ListReferenceCandidatesResponse,
  MemoryThreadMode,
  PermissionMode,
  StartRunRequest,
} from '@/shared/tauri/commands'
import { getCommandErrorMessage } from '@/shared/tauri/errors'
import { ComposerEditor } from './composer/ComposerEditor'
import { ComposerToolbar } from './composer/ComposerToolbar'
import type { ComposerDraft } from './composer/composer-draft-store'
import { clearDraft, getDraft, getEmptyDraft, saveDraft } from './composer/composer-draft-store'
import {
  flattenReferenceGroups,
  ReferenceCombobox,
  referenceGroups,
  referenceKey,
  referenceLabel,
} from './composer/ReferenceCombobox'
import { type SlashCommand, SlashCommandMenu, slashCommands } from './composer/SlashCommandMenu'

export type ComposerSubmitPayload = Omit<StartRunRequest, 'conversationId'>
export type ComposerMode =
  | { kind: 'ready' }
  | { kind: 'submitting' }
  | { kind: 'running-disabled'; canCancel?: boolean }
  | { kind: 'queue' }
  | { kind: 'clarification-reply' }
  | { kind: 'review-comment' }
  | { kind: 'retry' }
  | { kind: 'continue' }

type ComposerProps = {
  conversationId?: string
  draftKey?: string
  onSubmit: (draft: ComposerSubmitPayload) => Promise<void> | void
  mode?: ComposerMode
  pending?: boolean
  disabled?: boolean
  errorMessage?: string
  cancelPending?: boolean
  onCancelRun?: () => Promise<void> | void
  onRetry?: () => void
  onPickAttachmentPath?: (modalities: AttachmentInputModality[]) => Promise<string | null>
  onCreateAttachmentFromPath?: (
    path: string,
    conversationId?: string,
  ) => Promise<{ attachment: AttachmentReference }>
  onListReferenceCandidates?: () => Promise<ListReferenceCandidatesResponse>
  modelCapability?: ConversationModelCapability | null
  modelConfigDisabled?: boolean
  modelConfigId?: string
  submitModelConfigId?: string
  modelConfigs?: Array<{ id: string; label: string }>
  onModelConfigChange?: (modelConfigId: string) => void
  permissionMode?: PermissionMode
  autoModeAvailable?: boolean
  onPermissionModeChange?: (mode: PermissionMode) => void
  memoryMode?: MemoryThreadMode
  memoryModeDisabled?: boolean
  onMemoryModeChange?: (mode: MemoryThreadMode) => void
  submitAriaLabel?: string
  submitLabel?: string
}

const attachmentInputModalities: AttachmentInputModality[] = ['image', 'video', 'file']

const emptyReferenceCandidates: ListReferenceCandidatesResponse = {
  artifacts: [],
  conversations: [],
  files: [],
  memories: [],
  mcpServers: [],
  skills: [],
  tools: [],
}

export function Composer({
  conversationId,
  draftKey,
  onSubmit,
  mode,
  pending = false,
  disabled = false,
  errorMessage,
  cancelPending = false,
  onCancelRun,
  onRetry,
  onPickAttachmentPath,
  onCreateAttachmentFromPath,
  onListReferenceCandidates,
  modelCapability,
  modelConfigDisabled = false,
  modelConfigId,
  submitModelConfigId,
  modelConfigs = [],
  onModelConfigChange,
  permissionMode,
  autoModeAvailable = false,
  onPermissionModeChange,
  memoryMode,
  memoryModeDisabled = false,
  onMemoryModeChange,
  submitAriaLabel,
  submitLabel,
}: ComposerProps) {
  const { t } = useTranslation(['common', 'conversation'])
  const draftScope = draftKey ?? conversationId
  const [draftState, setDraftState] = useState<{ draft: ComposerDraft; scope?: string }>(() => ({
    draft: draftScope ? getDraft(draftScope) : getEmptyDraft(),
    scope: draftScope,
  }))
  const draft =
    draftState.scope === draftScope
      ? draftState.draft
      : draftScope
        ? getDraft(draftScope)
        : getEmptyDraft()
  const setDraft = (update: React.SetStateAction<ComposerDraft>) => {
    setDraftState((current) => {
      const currentDraft =
        current.scope === draftScope
          ? current.draft
          : draftScope
            ? getDraft(draftScope)
            : getEmptyDraft()
      return {
        draft: typeof update === 'function' ? update(currentDraft) : update,
        scope: draftScope,
      }
    })
  }
  useEffect(() => {
    if (draftScope) {
      saveDraft(draftScope, draft)
    }
  }, [draft, draftScope])
  const [composerError, setComposerError] = useState<string | null>(null)
  const [localPermissionMode, setLocalPermissionMode] = useState<PermissionMode>('default')
  const selectedPermissionMode = permissionMode ?? localPermissionMode
  const selectedModelConfigId = modelConfigId ?? ''
  const selectedSubmitModelConfigId = submitModelConfigId ?? selectedModelConfigId
  const effectiveMode = mode ?? fallbackComposerMode(pending, disabled)
  const editorDisabled =
    disabled || effectiveMode.kind === 'running-disabled' || (mode === undefined && pending)
  const submitDisabled = editorDisabled || effectiveMode.kind === 'submitting'
  const canSubmit = draft.text.trim().length > 0 && !submitDisabled
  const visibleError = composerError ?? errorMessage
  const canCancelRun =
    effectiveMode.kind === 'running-disabled' &&
    effectiveMode.canCancel !== false &&
    Boolean(onCancelRun)
  const acceptedAttachmentModalities = getAcceptedAttachmentModalities(modelCapability)
  const supportsAttachments = acceptedAttachmentModalities.length > 0
  const [slashOpen, setSlashOpen] = useState(false)
  const [slashActiveIndex, setSlashActiveIndex] = useState(0)
  const [referenceSource, setReferenceSource] = useState<'editor' | 'toolbar' | null>(null)
  const [referenceSearch, setReferenceSearch] = useState('')
  const [referenceCandidates, setReferenceCandidates] =
    useState<ListReferenceCandidatesResponse>(emptyReferenceCandidates)
  const [referenceLoading, setReferenceLoading] = useState(false)
  const [referenceActiveIndex, setReferenceActiveIndex] = useState(0)
  const referenceGroupItems = useMemo(
    () => referenceGroups(referenceCandidates, referenceSearch),
    [referenceCandidates, referenceSearch],
  )
  const referenceItems = useMemo(
    () => flattenReferenceGroups(referenceGroupItems),
    [referenceGroupItems],
  )

  async function submitDraft() {
    const submittedText = draft.text.trim()
    if (!submittedText || submitDisabled) {
      return
    }

    const payload: ComposerSubmitPayload = {
      attachments: draft.attachments,
      contextReferences: draft.contextReferences,
      permissionMode: selectedPermissionMode,
      prompt: submittedText,
    }
    if (selectedSubmitModelConfigId.length > 0) {
      payload.modelConfigId = selectedSubmitModelConfigId
    }

    try {
      await onSubmit(payload)
      setDraft(getEmptyDraft())
      if (draftScope) clearDraft(draftScope)
      setComposerError(null)
    } catch {
      // The parent owns the submitted error message. Keeping draft state is the important part here.
    }
  }

  async function handleAttachFile() {
    if (
      !onPickAttachmentPath ||
      !onCreateAttachmentFromPath ||
      editorDisabled ||
      !supportsAttachments
    ) {
      return
    }

    try {
      setComposerError(null)
      const path = await onPickAttachmentPath(acceptedAttachmentModalities)
      if (!path) {
        return
      }
      const { attachment } = await onCreateAttachmentFromPath(path, conversationId)
      setDraft((currentDraft) => ({
        ...currentDraft,
        attachments: addUniqueAttachment(currentDraft.attachments, attachment),
      }))
    } catch (error) {
      setComposerError(getCommandErrorMessage(error))
    }
  }

  async function handleCancelRun() {
    if (!onCancelRun || cancelPending) {
      return
    }

    try {
      setComposerError(null)
      await onCancelRun()
    } catch (error) {
      setComposerError(getCommandErrorMessage(error))
    }
  }

  function addContextReference(reference: ContextReference) {
    setDraft((currentDraft) => ({
      ...currentDraft,
      contextReferences: addUniqueReference(currentDraft.contextReferences, reference),
    }))
  }

  async function loadReferenceCandidates() {
    if (!onListReferenceCandidates) {
      setReferenceCandidates(emptyReferenceCandidates)
      return
    }

    setReferenceLoading(true)
    try {
      setReferenceCandidates(await onListReferenceCandidates())
    } finally {
      setReferenceLoading(false)
    }
  }

  function openReferenceCombobox(source: 'editor' | 'toolbar') {
    setSlashOpen(false)
    setReferenceSource(source)
    setReferenceActiveIndex(0)
    void loadReferenceCandidates()
  }

  function closeReferenceCombobox() {
    setReferenceSource(null)
  }

  function handleToolbarReferenceOpenChange(nextOpen: boolean) {
    if (nextOpen) {
      openReferenceCombobox('toolbar')
    } else if (referenceSource === 'toolbar') {
      closeReferenceCombobox()
    }
  }

  function selectReference(reference: ContextReference) {
    addContextReference(reference)
    closeReferenceCombobox()
    setReferenceSearch('')
    setReferenceActiveIndex(0)
    setDraft((currentDraft) => ({
      ...currentDraft,
      text: removeTrailingReferenceQuery(currentDraft.text),
    }))
  }

  function selectSlashCommand(command: SlashCommand) {
    setDraft((currentDraft) => ({
      ...currentDraft,
      text: replaceTrailingSlash(currentDraft.text, command.prompt),
    }))
    setSlashOpen(false)
    setSlashActiveIndex(0)
  }

  function handleTextChange(text: string) {
    setDraft((currentDraft) => ({
      ...currentDraft,
      text,
    }))
    if (shouldOpenSlashMenu(text)) {
      setSlashOpen(true)
      setSlashActiveIndex(0)
      closeReferenceCombobox()
      return
    }
    const referenceQuery = trailingReferenceQuery(text)
    if (referenceQuery !== null) {
      setReferenceSearch(referenceQuery)
      setReferenceActiveIndex(0)
      if (referenceSource !== 'editor') {
        openReferenceCombobox('editor')
      }
      setSlashOpen(false)
      return
    }
    setSlashOpen(false)
    if (referenceSource === 'editor') {
      closeReferenceCombobox()
    }
  }

  function handleEditorKeyCommand(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (slashOpen) {
      if (event.key === 'ArrowDown') {
        setSlashActiveIndex((index) => (index + 1) % slashCommands.length)
        return true
      }
      if (event.key === 'ArrowUp') {
        setSlashActiveIndex((index) => (index - 1 + slashCommands.length) % slashCommands.length)
        return true
      }
      if (event.key === 'Enter') {
        selectSlashCommand(slashCommands[slashActiveIndex] ?? slashCommands[0])
        return true
      }
      if (event.key === 'Escape') {
        setSlashOpen(false)
        return true
      }
    }

    if (referenceSource === 'editor') {
      if (event.key === 'ArrowDown') {
        setReferenceActiveIndex((index) =>
          referenceItems.length === 0 ? 0 : (index + 1) % referenceItems.length,
        )
        return true
      }
      if (event.key === 'ArrowUp') {
        setReferenceActiveIndex((index) =>
          referenceItems.length === 0
            ? 0
            : (index - 1 + referenceItems.length) % referenceItems.length,
        )
        return true
      }
      if (event.key === 'Enter') {
        const selectedReference = referenceItems[referenceActiveIndex]?.reference
        if (selectedReference) {
          selectReference(selectedReference)
        }
        return true
      }
      if (event.key === 'Escape') {
        closeReferenceCombobox()
        return true
      }
    }

    return false
  }

  function handleReferenceKeyCommand(event: KeyboardEvent<HTMLInputElement>) {
    if (!referenceSource) {
      return false
    }
    if (event.key === 'ArrowDown') {
      setReferenceActiveIndex((index) =>
        referenceItems.length === 0 ? 0 : (index + 1) % referenceItems.length,
      )
      return true
    }
    if (event.key === 'ArrowUp') {
      setReferenceActiveIndex((index) =>
        referenceItems.length === 0
          ? 0
          : (index - 1 + referenceItems.length) % referenceItems.length,
      )
      return true
    }
    if (event.key === 'Enter') {
      const selectedReference = referenceItems[referenceActiveIndex]?.reference
      if (selectedReference) {
        selectReference(selectedReference)
      }
      return true
    }
    if (event.key === 'Escape') {
      closeReferenceCombobox()
      return true
    }
    return false
  }

  return (
    <form
      className="rounded-md border border-border bg-surface px-3 py-2 shadow-sm focus-within:border-ring/60 focus-within:ring-2 focus-within:ring-ring/10 transition-[border-color,box-shadow] duration-300"
      onSubmit={(event) => {
        event.preventDefault()
        void submitDraft()
      }}
    >
      {visibleError ? (
        <div
          className="mb-3 flex items-center justify-between gap-3 rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm text-destructive"
          role="alert"
        >
          <span>{visibleError}</span>
          {onRetry && errorMessage ? (
            <button
              className="rounded-md border border-destructive/30 px-2 py-1 text-xs font-medium hover:bg-destructive/10 disabled:cursor-not-allowed disabled:opacity-60"
              disabled={submitDisabled}
              onClick={onRetry}
              type="button"
            >
              {t('common:retry')}
            </button>
          ) : null}
        </div>
      ) : null}

      <ComposerEditor
        disabled={editorDisabled}
        onChange={handleTextChange}
        onKeyCommand={handleEditorKeyCommand}
        onSubmit={() => {
          void submitDraft()
        }}
        value={draft.text}
      />
      <SlashCommandMenu
        activeIndex={slashActiveIndex}
        getCommandLabel={(command) => t(`conversation:composer.slashCommands.${command.id}`)}
        label={t('conversation:composer.slashCommands.label')}
        onSelect={selectSlashCommand}
        open={slashOpen}
      />
      <ReferenceCombobox
        activeIndex={referenceActiveIndex}
        disabled={editorDisabled}
        groups={referenceGroupItems}
        label={t('conversation:composer.referenceObject')}
        loadingLabel={t('conversation:composer.loadingReferences')}
        loading={referenceLoading}
        noResultsLabel={t('conversation:composer.noReferences')}
        onSearchChange={(search) => {
          setReferenceSearch(search)
          setReferenceActiveIndex(0)
        }}
        onSelectReference={selectReference}
        onKeyCommand={handleReferenceKeyCommand}
        open={referenceSource === 'editor'}
        search={referenceSearch}
        searchLabel={t('conversation:composer.searchReferences')}
      />

      <ComposerContextChips
        draft={draft}
        disabled={editorDisabled}
        onRemoveAttachment={(id) =>
          setDraft((currentDraft) => ({
            ...currentDraft,
            attachments: currentDraft.attachments.filter((attachment) => attachment.id !== id),
          }))
        }
        onRemoveReference={(key) =>
          setDraft((currentDraft) => ({
            ...currentDraft,
            contextReferences: currentDraft.contextReferences.filter(
              (reference) => referenceKey(reference) !== key,
            ),
          }))
        }
      />

      <div className="mt-1 flex items-center justify-between">
        <ComposerToolbar
          disabled={editorDisabled}
          supportsAttachments={supportsAttachments}
          onAttachFile={handleAttachFile}
          modelConfigDisabled={modelConfigDisabled}
          modelConfigId={modelConfigId}
          modelConfigs={modelConfigs}
          onModelConfigChange={onModelConfigChange}
          permissionMode={selectedPermissionMode}
          autoModeAvailable={autoModeAvailable}
          memoryMode={memoryMode}
          memoryModeDisabled={memoryModeDisabled}
          onMemoryModeChange={onMemoryModeChange}
          onPermissionModeChange={(nextMode) => {
            if (permissionMode === undefined) {
              setLocalPermissionMode(nextMode)
            }
            onPermissionModeChange?.(nextMode)
          }}
          referenceActiveIndex={referenceActiveIndex}
          referenceGroups={referenceGroupItems}
          referenceLoading={referenceLoading}
          referenceOpen={referenceSource === 'toolbar'}
          referenceSearch={referenceSearch}
          onReferenceOpenChange={handleToolbarReferenceOpenChange}
          onReferenceSearchChange={(search) => {
            setReferenceSearch(search)
            setReferenceActiveIndex(0)
          }}
          onReferenceKeyCommand={handleReferenceKeyCommand}
          onSelectReference={selectReference}
        />
        <div className="flex items-center gap-2">
          {canCancelRun ? (
            <button
              aria-label={t('conversation:composer.cancelRun')}
              className="rounded-md border border-border p-2 text-muted-foreground transition-[background-color,color,box-shadow,transform,opacity] duration-200 hover:bg-muted hover:text-foreground hover:shadow-sm active:scale-95 disabled:scale-100 disabled:opacity-50 disabled:pointer-events-none"
              disabled={cancelPending}
              onClick={() => {
                void handleCancelRun()
              }}
              type="button"
            >
              <X className="size-4" />
            </button>
          ) : null}
          <button
            aria-label={submitAriaLabel ?? t('conversation:composer.sendMessage')}
            className="inline-flex items-center gap-1.5 rounded-md bg-primary p-2 text-primary-foreground shadow-sm transition-[background-color,color,box-shadow,filter,transform] duration-200 hover:brightness-[1.05] hover:shadow-md active:scale-95 disabled:scale-100 disabled:bg-muted disabled:text-muted-foreground disabled:shadow-none disabled:pointer-events-none"
            disabled={!canSubmit}
            type="submit"
          >
            <Send className="size-4" />
            {submitLabel ? <span className="pr-0.5 font-medium text-xs">{submitLabel}</span> : null}
          </button>
        </div>
      </div>
    </form>
  )
}

function fallbackComposerMode(pending: boolean, disabled: boolean): ComposerMode {
  if (pending) {
    return { kind: 'submitting' }
  }
  if (disabled) {
    return { kind: 'running-disabled' }
  }
  return { kind: 'ready' }
}

function getAcceptedAttachmentModalities(
  modelCapability: ConversationModelCapability | null | undefined,
): AttachmentInputModality[] {
  if (modelCapability === null) {
    return []
  }
  if (!modelCapability) {
    return attachmentInputModalities
  }
  return attachmentInputModalities.filter((modality) =>
    modelCapability.inputModalities.includes(modality),
  )
}

function shouldOpenSlashMenu(text: string) {
  return /(^|\s)\/$/.test(text)
}

function trailingReferenceQuery(text: string) {
  const match = /(?:^|\s)@([^\s]*)$/.exec(text)
  return match ? match[1] : null
}

function replaceTrailingSlash(text: string, replacement: string) {
  return text.replace(/(^|\s)\/$/, (_match, prefix: string) => `${prefix}${replacement}`)
}

function removeTrailingReferenceQuery(text: string) {
  return text.replace(/(^|\s)@[^\s]*$/, '$1')
}

function ComposerContextChips({
  disabled,
  draft,
  onRemoveAttachment,
  onRemoveReference,
}: {
  disabled: boolean
  draft: ComposerDraft
  onRemoveAttachment: (id: string) => void
  onRemoveReference: (key: string) => void
}) {
  const { t } = useTranslation('conversation')
  const hasChips = draft.attachments.length > 0 || draft.contextReferences.length > 0

  if (!hasChips) {
    return null
  }

  return (
    <div className="mt-2 flex flex-wrap gap-1.5">
      {draft.contextReferences.map((reference) => (
        <ContextChip
          disabled={disabled}
          key={referenceKey(reference)}
          label={referenceLabel(reference)}
          onRemove={() => onRemoveReference(referenceKey(reference))}
          removeLabel={t('composer.removeReference', {
            label: referenceLabel(reference),
          })}
        />
      ))}
      {draft.attachments.map((attachment) => (
        <ContextChip
          disabled={disabled}
          key={attachment.id}
          label={attachment.name}
          onRemove={() => onRemoveAttachment(attachment.id)}
          removeLabel={t('composer.removeAttachment', {
            label: attachment.name,
          })}
        />
      ))}
    </div>
  )
}

function ContextChip({
  disabled,
  label,
  onRemove,
  removeLabel,
}: {
  disabled: boolean
  label: string
  onRemove: () => void
  removeLabel: string
}) {
  return (
    <span className="inline-flex max-w-full items-center gap-1 rounded-md border border-border bg-muted px-2 py-1 text-xs">
      <span className="truncate">{label}</span>
      <button
        aria-label={removeLabel}
        className="rounded-sm p-0.5 text-muted-foreground hover:bg-background hover:text-foreground disabled:cursor-not-allowed disabled:opacity-60"
        disabled={disabled}
        onClick={onRemove}
        type="button"
      >
        <X className="size-3" />
      </button>
    </span>
  )
}

function addUniqueAttachment(
  attachments: AttachmentReference[],
  attachment: AttachmentReference,
): AttachmentReference[] {
  return attachments.some((currentAttachment) => currentAttachment.id === attachment.id)
    ? attachments
    : [...attachments, attachment]
}

function addUniqueReference(
  references: ContextReference[],
  reference: ContextReference,
): ContextReference[] {
  return references.some(
    (currentReference) => referenceKey(currentReference) === referenceKey(reference),
  )
    ? references
    : [...references, reference]
}
