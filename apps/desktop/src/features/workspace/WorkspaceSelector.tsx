import { Check, ChevronDown, Folder } from 'lucide-react'
import { useState } from 'react'

import { cn } from '@/shared/lib/utils'

export type WorkspaceOption = {
  name: string
  path: string
  ref: string
}

type WorkspaceSelectorProps = {
  onSelect: (workspace: WorkspaceOption) => void
  selectedWorkspaceRef: string | null
  workspaces: WorkspaceOption[]
}

export function WorkspaceSelector({
  onSelect,
  selectedWorkspaceRef,
  workspaces,
}: WorkspaceSelectorProps) {
  const [open, setOpen] = useState(false)
  const selectedWorkspace =
    workspaces.find((workspace) => workspace.ref === selectedWorkspaceRef) ?? workspaces[0]

  return (
    <section className="px-3 pt-3" aria-label="Workspace selector">
      <fieldset
        aria-label={`Current workspace: ${selectedWorkspace.name}`}
        className="flex w-full items-center justify-between rounded-md px-2 py-2 text-left"
      >
        <span className="flex min-w-0 items-center gap-3">
          <span className="grid size-8 shrink-0 place-items-center rounded-md border border-border bg-surface text-foreground">
            <Folder aria-hidden="true" className="size-4" />
          </span>
          <span className="min-w-0">
            <span className="block truncate font-medium text-sm">{selectedWorkspace.name}</span>
            <span className="block truncate text-muted-foreground text-xs">
              {selectedWorkspace.path}
            </span>
          </span>
        </span>
        <button
          aria-expanded={open}
          aria-label="Choose workspace"
          className="rounded-md p-1 text-muted-foreground hover:bg-muted hover:text-foreground"
          onClick={() => setOpen((current) => !current)}
          type="button"
        >
          <ChevronDown className="size-4" />
        </button>
      </fieldset>

      {open ? (
        <ul aria-label="Available workspaces" className="mt-2 flex flex-col gap-1">
          {workspaces.map((workspace) => {
            const selected = workspace.ref === selectedWorkspace.ref

            return (
              <li key={workspace.ref}>
                <button
                  aria-current={selected ? 'true' : undefined}
                  aria-label={`${workspace.name} ${workspace.path}`}
                  className={cn(
                    'flex w-full items-center justify-between rounded-md px-2 py-1.5 text-left text-sm hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
                    selected ? 'bg-surface text-foreground' : 'text-muted-foreground',
                  )}
                  onClick={() => {
                    onSelect(workspace)
                    setOpen(false)
                  }}
                  type="button"
                >
                  <span className="min-w-0">
                    <span className="block truncate">{workspace.name}</span>
                    <span className="block truncate text-xs">{workspace.path}</span>
                  </span>
                  {selected ? <Check aria-hidden="true" className="size-4 shrink-0" /> : null}
                </button>
              </li>
            )
          })}
        </ul>
      ) : null}
    </section>
  )
}
