import { Check, ChevronDown, Paperclip, Send, Shield, X } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { cn } from '@/shared/lib/utils'
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
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/shared/ui/dropdown-menu'
import { Popover, PopoverContent, PopoverTrigger } from '@/shared/ui/popover'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/shared/ui/tooltip'
import { ComposerEditor } from './composer/ComposerEditor'
import type { ComposerDraft } from './composer/composer-draft-store'
import { clearDraft, getDraft, getEmptyDraft, saveDraft } from './composer/composer-draft-store'

export type ComposerSubmitPayload = Omit<StartRunRequest, 'conversationId'>
export type ComposerMode =
  | { kind: 'ready' }
  | { kind: 'submitting' }
  | { kind: 'running-disabled'; canCancel?: boolean }
  | { kind: 'clarification-reply' }
  | { kind: 'review-comment' }
  | { kind: 'retry' }
  | { kind: 'continue' }

type ComposerProps = {
  conversationId?: string
  onSubmit: (draft: ComposerSubmitPayload) => Promise<void> | void
  mode?: ComposerMode
  pending?: boolean
  disabled?: boolean
  errorMessage?: string
  cancelPending?: boolean
  onCancelRun?: () => Promise<void> | void
  onRetry?: () => void
  onPickAttachmentPath?: (modalities: AttachmentInputModality[]) => Promise<string | null>
  onCreateAttachmentFromPath?: (path: string) => Promise<{ attachment: AttachmentReference }>
  onListReferenceCandidates?: () => Promise<ListReferenceCandidatesResponse>
  modelCapability?: ConversationModelCapability | null
  modelConfigDisabled?: boolean
  modelConfigId?: string
  modelConfigs?: Array<{ id: string; label: string }>
  onModelConfigChange?: (modelConfigId: string) => void
  permissionMode?: PermissionMode
  autoModeAvailable?: boolean
  onPermissionModeChange?: (mode: PermissionMode) => void
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
  modelConfigs = [],
  onModelConfigChange,
  permissionMode,
  autoModeAvailable = false,
  onPermissionModeChange,
}: ComposerProps) {
  const { t } = useTranslation(['common', 'conversation'])
  const [draft, setDraft] = useState<ComposerDraft>(() =>
    conversationId ? getDraft(conversationId) : getEmptyDraft(),
  )
  useEffect(() => {
    if (conversationId) {
      saveDraft(conversationId, draft)
    }
  }, [conversationId, draft])
  const [composerError, setComposerError] = useState<string | null>(null)
  const [localPermissionMode, setLocalPermissionMode] = useState<PermissionMode>('default')
  const selectedPermissionMode = permissionMode ?? localPermissionMode
  const selectedModelConfigId = modelConfigId ?? ''
  const effectiveMode = mode ?? legacyComposerMode(pending, disabled)
  const isDisabled =
    disabled || effectiveMode.kind === 'submitting' || effectiveMode.kind === 'running-disabled'
  const canSubmit = draft.text.trim().length > 0 && selectedModelConfigId.length > 0 && !isDisabled
  const visibleError = composerError ?? errorMessage
  const canCancelRun =
    effectiveMode.kind === 'running-disabled' &&
    effectiveMode.canCancel !== false &&
    Boolean(onCancelRun)
  const acceptedAttachmentModalities = getAcceptedAttachmentModalities(modelCapability)
  const supportsAttachments = acceptedAttachmentModalities.length > 0

  async function submitDraft() {
    const submittedText = draft.text.trim()
    if (!submittedText || isDisabled) {
      return
    }

    const payload: ComposerSubmitPayload = {
      attachments: draft.attachments,
      contextReferences: draft.contextReferences,
      modelConfigId: selectedModelConfigId,
      permissionMode: selectedPermissionMode,
      prompt: submittedText,
    }

    try {
      await onSubmit(payload)
      setDraft(getEmptyDraft())
      if (conversationId) clearDraft(conversationId)
      setComposerError(null)
    } catch {
      // The parent owns the submitted error message. Keeping draft state is the important part here.
    }
  }

  async function handleAttachFile() {
    if (
      !onPickAttachmentPath ||
      !onCreateAttachmentFromPath ||
      isDisabled ||
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
      const { attachment } = await onCreateAttachmentFromPath(path)
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

  return (
    <form
      className="rounded-md border border-border bg-surface px-3 py-2 shadow-sm"
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
              disabled={isDisabled}
              onClick={onRetry}
              type="button"
            >
              {t('common:retry')}
            </button>
          ) : null}
        </div>
      ) : null}

      <ComposerEditor
        disabled={isDisabled}
        onChange={(text) =>
          setDraft((currentDraft) => ({
            ...currentDraft,
            text,
          }))
        }
        onSubmit={() => {
          void submitDraft()
        }}
        value={draft.text}
      />

      <ComposerContextChips
        draft={draft}
        disabled={isDisabled}
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
          disabled={isDisabled}
          supportsAttachments={supportsAttachments}
          onAttachFile={handleAttachFile}
          onListReferenceCandidates={onListReferenceCandidates}
          modelConfigDisabled={modelConfigDisabled}
          modelConfigId={modelConfigId}
          modelConfigs={modelConfigs}
          onModelConfigChange={onModelConfigChange}
          permissionMode={selectedPermissionMode}
          autoModeAvailable={autoModeAvailable}
          onPermissionModeChange={(nextMode) => {
            if (permissionMode === undefined) {
              setLocalPermissionMode(nextMode)
            }
            onPermissionModeChange?.(nextMode)
          }}
          onSelectReference={addContextReference}
        />
        <div className="flex items-center gap-2">
          {canCancelRun ? (
            <button
              aria-label={t('conversation:composer.cancelRun')}
              className="rounded-md border border-border p-2 text-muted-foreground hover:bg-muted disabled:cursor-not-allowed disabled:opacity-60"
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
            aria-label={t('conversation:composer.sendMessage')}
            className="rounded-md bg-primary p-2 text-primary-foreground disabled:cursor-not-allowed disabled:opacity-60"
            disabled={!canSubmit}
            type="submit"
          >
            <Send className="size-4" />
          </button>
        </div>
      </div>
    </form>
  )
}

function legacyComposerMode(pending: boolean, disabled: boolean): ComposerMode {
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

function ComposerToolbar({
  disabled,
  autoModeAvailable,
  modelConfigDisabled,
  modelConfigId,
  modelConfigs,
  permissionMode,
  supportsAttachments,
  onAttachFile,
  onListReferenceCandidates,
  onModelConfigChange,
  onPermissionModeChange,
  onSelectReference,
}: {
  disabled: boolean
  autoModeAvailable: boolean
  modelConfigDisabled: boolean
  modelConfigId?: string
  modelConfigs: Array<{ id: string; label: string }>
  permissionMode: PermissionMode
  supportsAttachments: boolean
  onAttachFile: () => void
  onListReferenceCandidates?: () => Promise<ListReferenceCandidatesResponse>
  onModelConfigChange?: (modelConfigId: string) => void
  onPermissionModeChange: (mode: PermissionMode) => void
  onSelectReference: (reference: ContextReference) => void
}) {
  const { t } = useTranslation('conversation')

  return (
    <TooltipProvider delayDuration={150}>
      <div className="flex flex-wrap items-center gap-2 text-muted-foreground">
        <ComposerPermissionModeMenu
          autoModeAvailable={autoModeAvailable}
          disabled={disabled}
          permissionMode={permissionMode}
          onPermissionModeChange={onPermissionModeChange}
        />
        <AttachmentPicker disabled={disabled || !supportsAttachments} onAttachFile={onAttachFile} />
        <ReferencePicker
          disabled={disabled}
          onListReferenceCandidates={onListReferenceCandidates}
          onSelectReference={onSelectReference}
        />
        {modelConfigs.length > 0 ? (
          <select
            aria-label={t('modelConfig')}
            className="h-8 max-w-[220px] rounded-md border border-border bg-background px-2 text-foreground text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-60"
            disabled={disabled || modelConfigDisabled || !onModelConfigChange}
            onChange={(event) => onModelConfigChange?.(event.target.value)}
            value={modelConfigId ?? ''}
          >
            <option value="">{t('noModelConfigSelected')}</option>
            {modelConfigs.map((modelConfig) => (
              <option key={modelConfig.id} value={modelConfig.id}>
                {modelConfig.label}
              </option>
            ))}
          </select>
        ) : null}
      </div>
    </TooltipProvider>
  )
}

const composerPermissionModeOptions = [
  {
    value: 'default',
    labelKey: 'composer.permissionMode.default.label',
    descriptionKey: 'composer.permissionMode.default.description',
  },
  {
    value: 'auto',
    labelKey: 'composer.permissionMode.auto.label',
    descriptionKey: 'composer.permissionMode.auto.description',
    unavailableDescriptionKey: 'composer.permissionMode.auto.unavailable',
  },
  {
    value: 'bypass_permissions',
    labelKey: 'composer.permissionMode.bypass.label',
    descriptionKey: 'composer.permissionMode.bypass.description',
  },
] as const satisfies ReadonlyArray<{
  value: PermissionMode
  labelKey: string
  descriptionKey: string
  unavailableDescriptionKey?: string
}>

function ComposerPermissionModeMenu({
  autoModeAvailable,
  disabled,
  permissionMode,
  onPermissionModeChange,
}: {
  autoModeAvailable: boolean
  disabled: boolean
  permissionMode: PermissionMode
  onPermissionModeChange: (mode: PermissionMode) => void
}) {
  const { t } = useTranslation('conversation')
  const selectedOption =
    composerPermissionModeOptions.find((option) => option.value === permissionMode) ??
    composerPermissionModeOptions[0]
  const selectedLabel = t(selectedOption.labelKey)
  const isBypassMode = permissionMode === 'bypass_permissions'

  return (
    <DropdownMenu>
      <Tooltip>
        <TooltipTrigger asChild>
          <DropdownMenuTrigger asChild>
            <button
              aria-label={t('composer.permissionMode.ariaLabel', { mode: selectedLabel })}
              className={cn(
                'inline-flex h-8 items-center gap-1.5 rounded-md border border-border bg-background px-2 font-medium text-foreground text-xs hover:bg-muted disabled:cursor-not-allowed disabled:opacity-60',
                isBypassMode && 'border-warning/40 bg-warning/10 text-warning hover:bg-warning/15',
              )}
              disabled={disabled}
              type="button"
            >
              <Shield className="size-3.5" />
              <span className="max-w-[8rem] truncate">{selectedLabel}</span>
              <ChevronDown className="size-3.5 opacity-70" />
            </button>
          </DropdownMenuTrigger>
        </TooltipTrigger>
        <TooltipContent>{t('composer.permissionMode.tooltip')}</TooltipContent>
      </Tooltip>
      <DropdownMenuContent align="start" className="w-72">
        {composerPermissionModeOptions.map((option) => {
          const optionDisabled = option.value === 'auto' && !autoModeAvailable
          const descriptionKey =
            optionDisabled && option.unavailableDescriptionKey
              ? option.unavailableDescriptionKey
              : option.descriptionKey

          return (
            <DropdownMenuItem
              className="items-start gap-3 py-2"
              disabled={optionDisabled}
              key={option.value}
              onSelect={() => {
                if (!optionDisabled) {
                  onPermissionModeChange(option.value)
                }
              }}
            >
              <Shield
                className={cn(
                  'mt-0.5 size-4 shrink-0 text-muted-foreground',
                  option.value === 'bypass_permissions' && 'text-warning',
                )}
              />
              <span className="min-w-0 flex-1 space-y-0.5">
                <span className="block font-medium text-foreground text-sm">
                  {t(option.labelKey)}
                </span>
                <span className="block whitespace-normal text-muted-foreground text-xs leading-5">
                  {t(descriptionKey)}
                </span>
              </span>
              {permissionMode === option.value ? (
                <Check className="mt-0.5 size-4 shrink-0 text-primary" />
              ) : null}
            </DropdownMenuItem>
          )
        })}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

function AttachmentPicker({
  disabled,
  onAttachFile,
}: {
  disabled: boolean
  onAttachFile: () => void
}) {
  const { t } = useTranslation('conversation')

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          aria-label={t('composer.attachFile')}
          className="rounded-md p-1.5 hover:bg-muted disabled:cursor-not-allowed disabled:opacity-60"
          disabled={disabled}
          onClick={onAttachFile}
          type="button"
        >
          <Paperclip className="size-4" />
        </button>
      </TooltipTrigger>
      <TooltipContent>{t('composer.attachFile')}</TooltipContent>
    </Tooltip>
  )
}

function ReferencePicker({
  disabled,
  onListReferenceCandidates,
  onSelectReference,
}: {
  disabled: boolean
  onListReferenceCandidates?: () => Promise<ListReferenceCandidatesResponse>
  onSelectReference: (reference: ContextReference) => void
}) {
  const { t } = useTranslation('conversation')
  const [open, setOpen] = useState(false)
  const [search, setSearch] = useState('')
  const [candidates, setCandidates] =
    useState<ListReferenceCandidatesResponse>(emptyReferenceCandidates)
  const [loading, setLoading] = useState(false)

  async function handleOpenChange(nextOpen: boolean) {
    setOpen(nextOpen)
    if (!nextOpen || !onListReferenceCandidates) {
      return
    }

    setLoading(true)
    try {
      setCandidates(await onListReferenceCandidates())
    } finally {
      setLoading(false)
    }
  }

  const groups = useMemo(() => referenceGroups(candidates, search), [candidates, search])
  const hasCandidates = groups.some((group) => group.items.length > 0)

  return (
    <Popover open={open} onOpenChange={handleOpenChange}>
      <Tooltip>
        <TooltipTrigger asChild>
          <PopoverTrigger asChild>
            <button
              aria-label={t('composer.referenceObject')}
              className="rounded-md px-2 py-1 font-mono text-sm hover:bg-muted disabled:cursor-not-allowed disabled:opacity-60"
              disabled={disabled}
              type="button"
            >
              @
            </button>
          </PopoverTrigger>
        </TooltipTrigger>
        <TooltipContent>{t('composer.referenceObject')}</TooltipContent>
      </Tooltip>
      <PopoverContent align="start" className="w-80 p-2">
        <input
          aria-label={t('composer.searchReferences')}
          className="mb-2 w-full rounded-md border border-border bg-background px-2 py-1.5 text-sm outline-none"
          onChange={(event) => setSearch(event.target.value)}
          placeholder={t('composer.searchReferences')}
          value={search}
        />
        <div className="max-h-72 overflow-auto">
          {loading ? (
            <p className="px-2 py-3 text-muted-foreground text-sm">
              {t('composer.loadingReferences')}
            </p>
          ) : null}
          {!loading && !hasCandidates ? (
            <p className="px-2 py-3 text-muted-foreground text-sm">{t('composer.noReferences')}</p>
          ) : null}
          {groups.map((group) =>
            group.items.length > 0 ? (
              <div className="py-1" key={group.label}>
                <p className="px-2 py-1 font-medium text-muted-foreground text-xs">{group.label}</p>
                {group.items.map((item) => (
                  <button
                    className="block w-full rounded-md px-2 py-1.5 text-left text-sm hover:bg-muted"
                    key={referenceKey(item.reference)}
                    onClick={() => {
                      onSelectReference(item.reference)
                      setOpen(false)
                    }}
                    type="button"
                  >
                    {item.label}
                  </button>
                ))}
              </div>
            ) : null,
          )}
        </div>
      </PopoverContent>
    </Popover>
  )
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

function referenceGroups(candidates: ListReferenceCandidatesResponse, search: string) {
  const query = search.trim().toLocaleLowerCase()
  const matches = (label: string) => !query || label.toLocaleLowerCase().includes(query)

  return [
    {
      label: 'Files',
      items: candidates.files
        .filter((candidate) => matches(candidate.label))
        .map((candidate) => ({
          label: candidate.label,
          reference: {
            kind: 'workspace_file',
            label: candidate.label,
            path: candidate.path ?? candidate.label,
          } satisfies ContextReference,
        })),
    },
    {
      label: 'Artifacts',
      items: candidates.artifacts
        .filter((candidate) => candidate.id && matches(candidate.label))
        .map((candidate) => ({
          label: candidate.label,
          reference: {
            id: candidate.id ?? '',
            kind: 'artifact',
            label: candidate.label,
          } satisfies ContextReference,
        })),
    },
    {
      label: 'Conversations',
      items: candidates.conversations
        .filter((candidate) => candidate.id && matches(candidate.label))
        .map((candidate) => ({
          label: candidate.label,
          reference: {
            id: candidate.id ?? '',
            kind: 'conversation',
            label: candidate.label,
          } satisfies ContextReference,
        })),
    },
    {
      label: 'Memories',
      items: candidates.memories
        .filter((candidate) => candidate.id && matches(candidate.label))
        .map((candidate) => ({
          label: candidate.label,
          reference: {
            id: candidate.id ?? '',
            kind: 'memory',
            label: candidate.label,
          } satisfies ContextReference,
        })),
    },
    {
      label: 'Skills',
      items: candidates.skills
        .filter((candidate) => candidate.id && matches(candidate.label))
        .map((candidate) => ({
          label: candidate.label,
          reference: {
            id: candidate.id ?? '',
            kind: 'skill',
            label: candidate.label,
          } satisfies ContextReference,
        })),
    },
    {
      label: 'Tools',
      items: candidates.tools
        .filter((candidate) => candidate.id && matches(candidate.label))
        .map((candidate) => ({
          label: candidate.label,
          reference: {
            id: candidate.id ?? '',
            kind: 'tool',
            label: candidate.label,
          } satisfies ContextReference,
        })),
    },
    {
      label: 'MCP Servers',
      items: candidates.mcpServers
        .filter((candidate) => candidate.id && matches(candidate.label))
        .map((candidate) => ({
          label: candidate.label,
          reference: {
            id: candidate.id ?? '',
            kind: 'mcp_server',
            label: candidate.label,
          } satisfies ContextReference,
        })),
    },
  ]
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

function referenceKey(reference: ContextReference) {
  if (reference.kind === 'workspace_file') {
    return `${reference.kind}:${reference.path}`
  }

  return `${reference.kind}:${reference.id}`
}

function referenceLabel(reference: ContextReference) {
  return reference.label
}
