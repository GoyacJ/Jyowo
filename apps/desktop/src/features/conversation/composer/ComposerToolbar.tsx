import { Check, ChevronDown, Database, Paperclip, Shield } from 'lucide-react'
import type { KeyboardEvent } from 'react'
import { useTranslation } from 'react-i18next'

import { cn } from '@/shared/lib/utils'
import type { ContextReference, MemoryThreadMode, PermissionMode } from '@/shared/tauri/commands'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/shared/ui/dropdown-menu'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/shared/ui/tooltip'

import { ReferenceCombobox, type ReferenceComboboxGroup } from './ReferenceCombobox'

export function ComposerToolbar({
  disabled,
  autoModeAvailable,
  modelConfigDisabled,
  modelConfigId,
  modelConfigs,
  permissionMode,
  memoryMode,
  memoryModeDisabled,
  referenceActiveIndex,
  referenceGroups,
  referenceLoading,
  referenceOpen,
  referenceSearch,
  supportsAttachments,
  onAttachFile,
  onModelConfigChange,
  onMemoryModeChange,
  onPermissionModeChange,
  onReferenceOpenChange,
  onReferenceSearchChange,
  onReferenceKeyCommand,
  onSelectReference,
}: {
  disabled: boolean
  autoModeAvailable: boolean
  modelConfigDisabled: boolean
  modelConfigId?: string
  modelConfigs: Array<{ id: string; label: string }>
  permissionMode: PermissionMode
  memoryMode?: MemoryThreadMode
  memoryModeDisabled: boolean
  referenceActiveIndex: number
  referenceGroups: ReferenceComboboxGroup[]
  referenceLoading: boolean
  referenceOpen: boolean
  referenceSearch: string
  supportsAttachments: boolean
  onAttachFile: () => void
  onModelConfigChange?: (modelConfigId: string) => void
  onMemoryModeChange?: (mode: MemoryThreadMode) => void
  onPermissionModeChange: (mode: PermissionMode) => void
  onReferenceOpenChange: (open: boolean) => void
  onReferenceSearchChange: (search: string) => void
  onReferenceKeyCommand: (event: KeyboardEvent<HTMLInputElement>) => boolean
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
        {memoryMode ? (
          <ComposerMemoryModeMenu
            disabled={disabled || memoryModeDisabled || !onMemoryModeChange}
            memoryMode={memoryMode}
            onMemoryModeChange={(nextMode) => onMemoryModeChange?.(nextMode)}
          />
        ) : null}
        <AttachmentPicker disabled={disabled || !supportsAttachments} onAttachFile={onAttachFile} />
        <ReferenceCombobox
          activeIndex={referenceActiveIndex}
          disabled={disabled}
          groups={referenceGroups}
          label={t('composer.referenceObject')}
          loadingLabel={t('composer.loadingReferences')}
          loading={referenceLoading}
          noResultsLabel={t('composer.noReferences')}
          onSearchChange={onReferenceSearchChange}
          onSelectReference={onSelectReference}
          onKeyCommand={onReferenceKeyCommand}
          open={referenceOpen}
          search={referenceSearch}
          searchLabel={t('composer.searchReferences')}
          trigger={
            <ReferenceTrigger
              disabled={disabled}
              label={t('composer.referenceObject')}
              onClick={() => onReferenceOpenChange(!referenceOpen)}
            />
          }
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

const composerMemoryModeOptions = [
  { value: 'read_write', labelKey: 'composer.memoryMode.readWrite' },
  { value: 'read_only', labelKey: 'composer.memoryMode.readOnly' },
  { value: 'candidate_only', labelKey: 'composer.memoryMode.candidateOnly' },
  { value: 'off', labelKey: 'composer.memoryMode.off' },
] as const satisfies ReadonlyArray<{
  value: MemoryThreadMode
  labelKey: string
}>

function ComposerMemoryModeMenu({
  disabled,
  memoryMode,
  onMemoryModeChange,
}: {
  disabled: boolean
  memoryMode: MemoryThreadMode
  onMemoryModeChange: (mode: MemoryThreadMode) => void
}) {
  const { t } = useTranslation('conversation')
  const selectedOption =
    composerMemoryModeOptions.find((option) => option.value === memoryMode) ??
    composerMemoryModeOptions[0]
  const selectedLabel = t(selectedOption.labelKey)

  return (
    <DropdownMenu>
      <Tooltip>
        <TooltipTrigger asChild>
          <DropdownMenuTrigger asChild>
            <button
              aria-label={t('composer.memoryMode.ariaLabel', { mode: selectedLabel })}
              className="inline-flex h-8 items-center gap-1.5 rounded-md border border-border bg-background px-2 font-medium text-foreground text-xs hover:bg-muted disabled:cursor-not-allowed disabled:opacity-60"
              disabled={disabled}
              type="button"
            >
              <Database className="size-3.5" />
              <span className="max-w-[8rem] truncate">{selectedLabel}</span>
              <ChevronDown className="size-3.5 opacity-70" />
            </button>
          </DropdownMenuTrigger>
        </TooltipTrigger>
        <TooltipContent>{t('composer.memoryMode.tooltip')}</TooltipContent>
      </Tooltip>
      <DropdownMenuContent align="start" className="w-56">
        {composerMemoryModeOptions.map((option) => (
          <DropdownMenuItem
            className="items-center gap-3 py-2"
            key={option.value}
            onSelect={() => onMemoryModeChange(option.value)}
          >
            <Check
              className={cn('size-3.5', option.value === memoryMode ? 'opacity-100' : 'opacity-0')}
            />
            <span>{t(option.labelKey)}</span>
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
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

function ReferenceTrigger({
  disabled,
  label,
  onClick,
}: {
  disabled: boolean
  label: string
  onClick: () => void
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          aria-label={label}
          className="rounded-md px-2 py-1 font-mono text-sm hover:bg-muted disabled:cursor-not-allowed disabled:opacity-60"
          disabled={disabled}
          onClick={onClick}
          type="button"
        >
          @
        </button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  )
}
