import { LoaderCircle, Pause, Play, Send, X } from 'lucide-react'
import type { KeyboardEvent } from 'react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { MemoryThreadMode } from '@/generated/daemon-protocol'
import type {
  AttachmentInputModality,
  AttachmentReference,
  ContextReference,
  ConversationModelCapability,
  ListReferenceCandidatesResponse,
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
  type ReferenceGroupId,
  referenceGroups,
  referenceKey,
  referenceLabel,
  referenceListboxId,
  referenceOptionId,
} from './composer/ReferenceCombobox'
import {
  type SlashCommand,
  SlashCommandMenu,
  slashCommandListboxId,
  slashCommandOptionId,
  slashCommands,
} from './composer/SlashCommandMenu'

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
  pausePending?: boolean
  onPauseRun?: () => Promise<void> | void
  continuePending?: boolean
  onContinueRun?: () => Promise<void> | void
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

type ComposerInlineTrigger = {
  end: number
  kind: 'reference' | 'slash'
  query: string
  start: number
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
  pausePending = false,
  onPauseRun,
  continuePending = false,
  onContinueRun,
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
  const draftRef = useRef(draft)
  draftRef.current = draft
  const setDraft = (update: React.SetStateAction<ComposerDraft>) => {
    setDraftState((current) => {
      const currentDraft =
        current.scope === draftScope
          ? current.draft
          : draftScope
            ? getDraft(draftScope)
            : getEmptyDraft()
      const nextDraft = typeof update === 'function' ? update(currentDraft) : update
      draftRef.current = nextDraft
      return {
        draft: nextDraft,
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
  const formRef = useRef<HTMLFormElement>(null)
  const editorRef = useRef<HTMLTextAreaElement>(null)
  const [slashTrigger, setSlashTrigger] = useState<ComposerInlineTrigger | null>(null)
  const [slashActiveIndex, setSlashActiveIndex] = useState(0)
  const [referenceSource, setReferenceSource] = useState<'editor' | 'toolbar' | null>(null)
  const [referenceTrigger, setReferenceTrigger] = useState<ComposerInlineTrigger | null>(null)
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
  const filteredSlashCommands = useMemo(() => {
    const query = slashTrigger?.query.trim().toLocaleLowerCase() ?? ''
    if (!query) {
      return slashCommands
    }

    return slashCommands.filter((command) =>
      [
        command.id,
        t(`conversation:composer.slashCommands.${command.id}`),
        t(`conversation:composer.slashCommands.descriptions.${command.id}`),
      ].some((value) => value.toLocaleLowerCase().includes(query)),
    )
  }, [slashTrigger?.query, t])
  const slashOpen = slashTrigger !== null
  const activeSlashCommand = filteredSlashCommands[slashActiveIndex]
  const activeReference = referenceItems[referenceActiveIndex]?.reference
  const suggestionControls = slashOpen
    ? slashCommandListboxId
    : referenceSource === 'editor'
      ? referenceListboxId
      : undefined
  const suggestionActiveDescendant =
    slashOpen && activeSlashCommand
      ? slashCommandOptionId(activeSlashCommand)
      : referenceSource === 'editor' && activeReference
        ? referenceOptionId(activeReference)
        : undefined

  useEffect(() => {
    if (!slashOpen && referenceSource === null) {
      return
    }

    function handlePointerDown(event: PointerEvent) {
      if (!formRef.current?.contains(event.target as Node)) {
        setSlashTrigger(null)
        setReferenceSource(null)
        setReferenceTrigger(null)
      }
    }

    document.addEventListener('pointerdown', handlePointerDown)
    return () => document.removeEventListener('pointerdown', handlePointerDown)
  }, [referenceSource, slashOpen])

  async function submitDraft() {
    const submittedDraft = draft
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
      if (draftRef.current === submittedDraft) {
        setDraft(getEmptyDraft())
        if (draftScope) clearDraft(draftScope)
      } else {
        const submittedAttachmentIds = new Set(
          submittedDraft.attachments.map((attachment) => attachment.id),
        )
        const submittedReferenceKeys = new Set(submittedDraft.contextReferences.map(referenceKey))
        setDraft((current) => ({
          attachments: current.attachments.filter(
            (attachment) => !submittedAttachmentIds.has(attachment.id),
          ),
          contextReferences: current.contextReferences.filter(
            (reference) => !submittedReferenceKeys.has(referenceKey(reference)),
          ),
          text: current.text === submittedDraft.text ? '' : current.text,
        }))
      }
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

  async function handlePauseRun() {
    if (!onPauseRun || pausePending) return
    try {
      setComposerError(null)
      await onPauseRun()
    } catch (error) {
      setComposerError(getCommandErrorMessage(error))
    }
  }

  async function handleContinueRun() {
    if (!onContinueRun || continuePending) return
    try {
      setComposerError(null)
      await onContinueRun()
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

  function openReferenceCombobox(
    source: 'editor' | 'toolbar',
    trigger: ComposerInlineTrigger | null = null,
  ) {
    setSlashTrigger(null)
    setReferenceSource(source)
    setReferenceTrigger(trigger)
    setReferenceActiveIndex(0)
    if (source === 'toolbar') {
      setReferenceSearch('')
    }
    void loadReferenceCandidates()
  }

  function closeReferenceCombobox() {
    setReferenceSource(null)
    setReferenceTrigger(null)
  }

  function handleToolbarReferenceOpenChange(nextOpen: boolean) {
    if (nextOpen) {
      openReferenceCombobox('toolbar')
    } else if (referenceSource === 'toolbar') {
      closeReferenceCombobox()
    }
  }

  function selectReference(reference: ContextReference) {
    const selectedTrigger = referenceSource === 'editor' ? referenceTrigger : null
    addContextReference(reference)
    closeReferenceCombobox()
    setReferenceSearch('')
    setReferenceActiveIndex(0)
    setDraft((currentDraft) => ({
      ...currentDraft,
      text: selectedTrigger
        ? replaceComposerTrigger(currentDraft.text, selectedTrigger, '')
        : currentDraft.text,
    }))
    restoreEditorFocus(selectedTrigger?.start)
  }

  function selectSlashCommand(command: SlashCommand) {
    const selectedTrigger = slashTrigger
    if (!selectedTrigger) {
      return
    }

    setDraft((currentDraft) => ({
      ...currentDraft,
      text: replaceComposerTrigger(currentDraft.text, selectedTrigger, command.prompt),
    }))
    setSlashTrigger(null)
    setSlashActiveIndex(0)
    restoreEditorFocus(selectedTrigger.start + command.prompt.length)
  }

  function restoreEditorFocus(cursorPosition?: number) {
    requestAnimationFrame(() => {
      editorRef.current?.focus()
      if (cursorPosition !== undefined) {
        editorRef.current?.setSelectionRange(cursorPosition, cursorPosition)
      }
    })
  }

  function syncInlineSuggestion(text: string, cursorPosition: number) {
    const trigger = composerInlineTrigger(text, cursorPosition)

    if (trigger?.kind === 'slash') {
      setSlashTrigger(trigger)
      setSlashActiveIndex(0)
      if (referenceSource === 'editor') {
        closeReferenceCombobox()
      }
      return
    }

    if (trigger?.kind === 'reference') {
      setReferenceSearch(trigger.query)
      setReferenceActiveIndex(0)
      setSlashTrigger(null)
      if (referenceSource !== 'editor') {
        openReferenceCombobox('editor', trigger)
      } else {
        setReferenceTrigger(trigger)
      }
      return
    }

    setSlashTrigger(null)
    if (referenceSource === 'editor') {
      closeReferenceCombobox()
    }
  }

  function handleTextChange(text: string, cursorPosition: number) {
    setDraft((currentDraft) => ({
      ...currentDraft,
      text,
    }))
    syncInlineSuggestion(text, cursorPosition)
  }

  function handleEditorKeyCommand(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (slashOpen) {
      if (event.key === 'ArrowDown') {
        setSlashActiveIndex((index) =>
          filteredSlashCommands.length === 0 ? 0 : (index + 1) % filteredSlashCommands.length,
        )
        return true
      }
      if (event.key === 'ArrowUp') {
        setSlashActiveIndex((index) =>
          filteredSlashCommands.length === 0
            ? 0
            : (index - 1 + filteredSlashCommands.length) % filteredSlashCommands.length,
        )
        return true
      }
      if (event.key === 'Enter' || event.key === 'Tab') {
        const command = filteredSlashCommands[slashActiveIndex]
        if (command) {
          selectSlashCommand(command)
        }
        return true
      }
      if (event.key === 'Escape') {
        setSlashTrigger(null)
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
      if (event.key === 'Enter' || event.key === 'Tab') {
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
    if (event.key === 'Enter' || event.key === 'Tab') {
      const selectedReference = referenceItems[referenceActiveIndex]?.reference
      if (selectedReference) {
        selectReference(selectedReference)
      }
      return true
    }
    if (event.key === 'Escape') {
      closeReferenceCombobox()
      restoreEditorFocus()
      return true
    }
    return false
  }

  return (
    <form
      ref={formRef}
      className="relative rounded-md border border-border bg-surface px-3 py-2 shadow-sm focus-within:border-ring/60 focus-within:ring-2 focus-within:ring-ring/10 transition-[border-color,box-shadow] duration-300"
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
        ref={editorRef}
        activeDescendant={suggestionActiveDescendant}
        controls={suggestionControls}
        disabled={editorDisabled}
        expanded={slashOpen || referenceSource === 'editor'}
        onChange={handleTextChange}
        onCursorChange={(cursorPosition) => syncInlineSuggestion(draft.text, cursorPosition)}
        onKeyCommand={handleEditorKeyCommand}
        onSubmit={() => {
          void submitDraft()
        }}
        value={draft.text}
      />
      <SlashCommandMenu
        activeIndex={slashActiveIndex}
        commands={filteredSlashCommands}
        emptyLabel={t('conversation:composer.slashCommands.empty')}
        getCommandDescription={(command) =>
          t(`conversation:composer.slashCommands.descriptions.${command.id}`)
        }
        getCommandLabel={(command) => t(`conversation:composer.slashCommands.${command.id}`)}
        keyboardHint={t('conversation:composer.suggestionKeyboardHint')}
        label={t('conversation:composer.slashCommands.label')}
        onSelect={selectSlashCommand}
        open={slashOpen}
      />
      <ReferenceCombobox
        activeIndex={referenceActiveIndex}
        disabled={editorDisabled}
        getGroupLabel={(groupId: ReferenceGroupId) =>
          t(`conversation:composer.referenceGroups.${groupId}`)
        }
        groups={referenceGroupItems}
        keyboardHint={t('conversation:composer.suggestionKeyboardHint')}
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
        open={referenceSource !== null}
        resultCountLabel={(count) => t('conversation:composer.referenceResultCount', { count })}
        search={referenceSearch}
        searchInputVisible={referenceSource === 'toolbar'}
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

      <div className="mt-1 flex min-w-0 items-center justify-between gap-1.5">
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
          referenceOpen={referenceSource === 'toolbar'}
          onReferenceOpenChange={handleToolbarReferenceOpenChange}
        />
        <div className="flex shrink-0 items-center gap-1.5">
          {onPauseRun ? (
            <button
              aria-label={t(
                pausePending
                  ? 'conversation:composer.pausingRun'
                  : 'conversation:composer.pauseRun',
              )}
              className="inline-flex items-center gap-1.5 rounded-md border border-border px-2.5 py-2 font-medium text-muted-foreground text-xs transition-[background-color,color,box-shadow,transform,opacity] duration-200 hover:bg-muted hover:text-foreground hover:shadow-sm active:scale-95 disabled:scale-100 disabled:opacity-50 disabled:pointer-events-none"
              disabled={pausePending}
              onClick={() => {
                void handlePauseRun()
              }}
              type="button"
            >
              {pausePending ? (
                <LoaderCircle
                  aria-hidden="true"
                  className="size-4 animate-spin motion-reduce:animate-none"
                />
              ) : (
                <Pause aria-hidden="true" className="size-4" />
              )}
              <span className="task-composer-run-control-label">
                {t(
                  pausePending
                    ? 'conversation:composer.pausingRun'
                    : 'conversation:composer.pauseRun',
                )}
              </span>
            </button>
          ) : null}
          {onContinueRun ? (
            <button
              aria-label={t(
                continuePending
                  ? 'conversation:composer.continuingRun'
                  : 'conversation:composer.continueRun',
              )}
              className="inline-flex items-center gap-1.5 rounded-md border border-border px-2.5 py-2 font-medium text-muted-foreground text-xs transition-[background-color,color,box-shadow,transform,opacity] duration-200 hover:bg-muted hover:text-foreground hover:shadow-sm active:scale-95 disabled:scale-100 disabled:opacity-50 disabled:pointer-events-none"
              disabled={continuePending}
              onClick={() => {
                void handleContinueRun()
              }}
              type="button"
            >
              {continuePending ? (
                <LoaderCircle
                  aria-hidden="true"
                  className="size-4 animate-spin motion-reduce:animate-none"
                />
              ) : (
                <Play aria-hidden="true" className="size-4" />
              )}
              <span className="task-composer-run-control-label">
                {t(
                  continuePending
                    ? 'conversation:composer.continuingRun'
                    : 'conversation:composer.continueRun',
                )}
              </span>
            </button>
          ) : null}
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

function composerInlineTrigger(text: string, cursorPosition: number): ComposerInlineTrigger | null {
  const boundedCursor = Math.min(Math.max(cursorPosition, 0), text.length)
  const beforeCursor = text.slice(0, boundedCursor)
  const match = /(^|\s)([/@])([^\s]*)$/.exec(beforeCursor)
  if (!match) {
    return null
  }

  const triggerCharacter = match[2]
  const start = match.index + (match[1]?.length ?? 0)
  const suffixLength = /^[^\s]*/.exec(text.slice(boundedCursor))?.[0].length ?? 0
  const end = boundedCursor + suffixLength

  return {
    end,
    kind: triggerCharacter === '/' ? 'slash' : 'reference',
    query: text.slice(start + 1, end),
    start,
  }
}

function replaceComposerTrigger(text: string, trigger: ComposerInlineTrigger, replacement: string) {
  const prefix = text.slice(0, trigger.start)
  let suffix = text.slice(trigger.end)
  if (suffix.startsWith(' ') && (prefix.endsWith(' ') || replacement.endsWith(' '))) {
    suffix = suffix.slice(1)
  }
  return `${prefix}${replacement}${suffix}`
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
