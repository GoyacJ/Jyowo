import type { LucideIcon } from 'lucide-react'
import {
  Brain,
  File,
  FileBox,
  LoaderCircle,
  MessageSquare,
  PlugZap,
  SearchX,
  Sparkles,
  Wrench,
} from 'lucide-react'
import { type KeyboardEvent, useEffect, useRef } from 'react'

import { cn } from '@/shared/lib/utils'
import type { ContextReference, ListReferenceCandidatesResponse } from '@/shared/tauri/commands'
import { Input } from '@/shared/ui/input'

import { ComposerSuggestionPanel } from './ComposerSuggestionPanel'

type ReferenceComboboxItem = {
  description?: string
  label: string
  reference: ContextReference
}

export type ReferenceGroupId =
  | 'files'
  | 'artifacts'
  | 'conversations'
  | 'memories'
  | 'skills'
  | 'tools'
  | 'mcpServers'

export type ReferenceComboboxGroup = {
  id: ReferenceGroupId
  items: ReferenceComboboxItem[]
}

const groupIcons: Record<ReferenceGroupId, LucideIcon> = {
  artifacts: FileBox,
  conversations: MessageSquare,
  files: File,
  mcpServers: PlugZap,
  memories: Brain,
  skills: Sparkles,
  tools: Wrench,
}

export const referenceListboxId = 'composer-reference-combobox-listbox'

export function ReferenceCombobox({
  activeIndex,
  disabled,
  getGroupLabel,
  groups,
  keyboardHint,
  label,
  loadingLabel,
  loading,
  noResultsLabel,
  onSearchChange,
  onSelectReference,
  onKeyCommand,
  open,
  resultCountLabel,
  search,
  searchInputVisible,
  searchLabel,
}: {
  activeIndex: number
  disabled?: boolean
  getGroupLabel: (groupId: ReferenceGroupId) => string
  groups: ReferenceComboboxGroup[]
  keyboardHint: string
  label: string
  loadingLabel: string
  loading: boolean
  noResultsLabel: string
  onSearchChange: (search: string) => void
  onSelectReference: (reference: ContextReference) => void
  onKeyCommand?: (event: KeyboardEvent<HTMLInputElement>) => boolean
  open: boolean
  resultCountLabel: (count: number) => string
  search: string
  searchInputVisible: boolean
  searchLabel: string
}) {
  const items = flattenReferenceGroups(groups)
  const hasCandidates = items.length > 0
  const activeItem = items[activeIndex]
  const activeItemId = activeItem ? referenceOptionId(activeItem.reference) : undefined
  const searchInputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (open && searchInputVisible) {
      searchInputRef.current?.focus()
    }
  }, [open, searchInputVisible])

  if (!open) {
    return null
  }

  return (
    <ComposerSuggestionPanel keyboardHint={keyboardHint}>
      {searchInputVisible ? (
        <div className="border-border border-b p-2">
          <Input
            ref={searchInputRef}
            aria-activedescendant={activeItemId}
            aria-autocomplete="list"
            aria-controls={referenceListboxId}
            aria-expanded={open}
            aria-label={searchLabel}
            disabled={disabled}
            onKeyDown={(event) => {
              if (onKeyCommand?.(event)) {
                event.preventDefault()
                event.stopPropagation()
              }
            }}
            onChange={(event) => onSearchChange(event.target.value)}
            placeholder={searchLabel}
            role="combobox"
            value={search}
          />
        </div>
      ) : null}
      <div
        aria-label={label}
        className="max-h-[min(360px,45vh)] overflow-y-auto p-1.5"
        id={referenceListboxId}
        role="listbox"
      >
        <p aria-live="polite" className="sr-only">
          {loading ? loadingLabel : resultCountLabel(items.length)}
        </p>
        {loading ? (
          <div
            className="flex items-center gap-2 px-3 py-5 text-muted-foreground text-sm"
            role="status"
          >
            <LoaderCircle
              aria-hidden="true"
              className="size-4 shrink-0 animate-spin motion-reduce:animate-none"
            />
            <span>{loadingLabel}</span>
          </div>
        ) : null}
        {!loading && !hasCandidates ? (
          <div className="flex items-center gap-2 px-3 py-5 text-muted-foreground text-sm">
            <SearchX aria-hidden="true" className="size-4 shrink-0" />
            <span>{noResultsLabel}</span>
          </div>
        ) : null}
        {!loading
          ? groups.map((group) => {
              if (group.items.length === 0) {
                return null
              }

              const Icon = groupIcons[group.id]

              return (
                <div className="py-1" key={group.id}>
                  <p className="px-3 py-1.5 font-medium text-muted-foreground text-xs">
                    {getGroupLabel(group.id)}
                  </p>
                  {group.items.map((item) => {
                    const itemIndex = items.findIndex(
                      (currentItem) =>
                        referenceKey(currentItem.reference) === referenceKey(item.reference),
                    )
                    const selected = itemIndex === activeIndex

                    return (
                      <button
                        aria-label={item.label}
                        aria-selected={selected}
                        className={cn(
                          'flex min-h-11 w-full items-center gap-3 rounded-md px-3 py-2 text-left outline-none transition-colors hover:bg-muted',
                          selected && 'bg-muted text-foreground',
                        )}
                        id={referenceOptionId(item.reference)}
                        key={referenceKey(item.reference)}
                        onClick={() => onSelectReference(item.reference)}
                        role="option"
                        type="button"
                      >
                        <Icon
                          aria-hidden="true"
                          className="size-4 shrink-0 text-muted-foreground"
                        />
                        <span className="min-w-0 flex-1">
                          <span className="block truncate text-foreground text-sm">
                            {item.label}
                          </span>
                          {item.description ? (
                            <span className="block truncate text-muted-foreground text-xs">
                              {item.description}
                            </span>
                          ) : null}
                        </span>
                      </button>
                    )
                  })}
                </div>
              )
            })
          : null}
      </div>
    </ComposerSuggestionPanel>
  )
}

export function referenceGroups(
  candidates: ListReferenceCandidatesResponse,
  search: string,
): ReferenceComboboxGroup[] {
  const query = search.trim().toLocaleLowerCase()
  const matches = (...values: Array<string | undefined>) =>
    !query || values.some((value) => value?.toLocaleLowerCase().includes(query))

  return [
    {
      id: 'files',
      items: candidates.files
        .filter((candidate) => matches(candidate.label, candidate.path))
        .map((candidate) => {
          const path = candidate.path ?? candidate.label
          const label = fileName(candidate.label)
          return {
            description: path !== label ? path : undefined,
            label,
            reference: {
              kind: 'workspace_file',
              label: candidate.label,
              path,
            } satisfies ContextReference,
          }
        }),
    },
    {
      id: 'artifacts',
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
      id: 'conversations',
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
      id: 'memories',
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
      id: 'skills',
      items: candidates.skills
        .filter(
          (candidate) =>
            candidate.id && matches(candidate.label, skillSourceDescription(candidate.source)),
        )
        .map((candidate) => ({
          description: skillSourceDescription(candidate.source),
          label: candidate.label,
          reference: {
            kind: 'skill',
            label: candidate.label,
            parameters: {},
            skillId: candidate.id ?? '',
            source: candidate.source,
            version: 1,
          } satisfies ContextReference,
        })),
    },
    {
      id: 'tools',
      items: candidates.tools
        .filter((candidate) => candidate.id && matches(candidate.label, candidate.id))
        .map((candidate) => ({
          description: candidate.id,
          label: candidate.label,
          reference: {
            id: candidate.id ?? '',
            kind: 'tool',
            label: candidate.label,
          } satisfies ContextReference,
        })),
    },
    {
      id: 'mcpServers',
      items: candidates.mcpServers
        .filter((candidate) => candidate.id && matches(candidate.label, candidate.id))
        .map((candidate) => ({
          description: candidate.id,
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

export function flattenReferenceGroups(groups: ReferenceComboboxGroup[]) {
  return groups.flatMap((group) => group.items)
}

export function referenceKey(reference: ContextReference) {
  if (reference.kind === 'workspace_file') {
    return `${reference.kind}:${reference.path}`
  }
  if (reference.kind === 'skill') {
    return `${reference.kind}:${reference.skillId}`
  }

  return `${reference.kind}:${reference.id}`
}

export function referenceOptionId(reference: ContextReference) {
  return `composer-reference-option-${referenceKey(reference).replace(/[^a-zA-Z0-9_-]/g, '-')}`
}

export function referenceLabel(reference: ContextReference) {
  return reference.label
}

function skillSourceDescription(
  source: ListReferenceCandidatesResponse['skills'][number]['source'],
) {
  if (typeof source === 'string') {
    return source
  }
  if ('plugin' in source) {
    return source.plugin
  }
  return source.mcp
}

function fileName(path: string) {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path
}
