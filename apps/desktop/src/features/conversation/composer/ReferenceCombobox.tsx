import type { KeyboardEvent, ReactNode } from 'react'

import { cn } from '@/shared/lib/utils'
import type { ContextReference, ListReferenceCandidatesResponse } from '@/shared/tauri/commands'
import { Input } from '@/shared/ui/input'

type ReferenceComboboxItem = {
  label: string
  reference: ContextReference
}

export type ReferenceComboboxGroup = {
  label: string
  items: ReferenceComboboxItem[]
}

export function ReferenceCombobox({
  activeIndex,
  disabled,
  groups,
  label,
  loadingLabel,
  loading,
  noResultsLabel,
  onSearchChange,
  onSelectReference,
  onKeyCommand,
  open,
  search,
  searchLabel,
  trigger,
}: {
  activeIndex: number
  disabled?: boolean
  groups: ReferenceComboboxGroup[]
  label: string
  loadingLabel: string
  loading: boolean
  noResultsLabel: string
  onSearchChange: (search: string) => void
  onSelectReference: (reference: ContextReference) => void
  onKeyCommand?: (event: KeyboardEvent<HTMLInputElement>) => boolean
  open: boolean
  search: string
  searchLabel: string
  trigger?: ReactNode
}) {
  const items = flattenReferenceGroups(groups)
  const hasCandidates = items.length > 0
  const listboxId = 'composer-reference-combobox-listbox'
  const activeItem = items[activeIndex]
  const activeItemId = activeItem
    ? `composer-reference-option-${referenceDomId(activeItem.reference)}`
    : undefined

  return (
    <div className="relative">
      {trigger ? trigger : null}
      {open ? (
        <div className="mt-2 w-80 rounded-md border border-border bg-popover p-2 text-popover-foreground shadow-md">
          <Input
            aria-activedescendant={activeItemId}
            aria-autocomplete="list"
            aria-controls={listboxId}
            aria-expanded={open}
            aria-label={searchLabel}
            className="mb-2"
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
          <div aria-label={label} className="max-h-72 overflow-auto" id={listboxId} role="listbox">
            {loading ? (
              <p className="px-2 py-3 text-muted-foreground text-sm">{loadingLabel}</p>
            ) : null}
            {!loading && !hasCandidates ? (
              <p className="px-2 py-3 text-muted-foreground text-sm">{noResultsLabel}</p>
            ) : null}
            {groups.map((group) =>
              group.items.length > 0 ? (
                <div className="py-1" key={group.label}>
                  <p className="px-2 py-1 font-medium text-muted-foreground text-xs">
                    {group.label}
                  </p>
                  {group.items.map((item) => {
                    const itemIndex = items.findIndex(
                      (currentItem) =>
                        referenceKey(currentItem.reference) === referenceKey(item.reference),
                    )
                    return (
                      <button
                        aria-selected={itemIndex === activeIndex}
                        className={cn(
                          'block w-full rounded-md px-2 py-1.5 text-left text-sm hover:bg-muted',
                          itemIndex === activeIndex && 'bg-muted',
                        )}
                        id={`composer-reference-option-${referenceDomId(item.reference)}`}
                        key={referenceKey(item.reference)}
                        onClick={() => onSelectReference(item.reference)}
                        role="option"
                        type="button"
                      >
                        {item.label}
                      </button>
                    )
                  })}
                </div>
              ) : null,
            )}
          </div>
        </div>
      ) : null}
    </div>
  )
}

export function referenceGroups(
  candidates: ListReferenceCandidatesResponse,
  search: string,
): ReferenceComboboxGroup[] {
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
            kind: 'skill',
            label: candidate.label,
            parameters: {},
            skillId: candidate.id ?? '',
            version: 1,
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

function referenceDomId(reference: ContextReference) {
  return referenceKey(reference).replace(/[^a-zA-Z0-9_-]/g, '-')
}

export function referenceLabel(reference: ContextReference) {
  return reference.label
}
