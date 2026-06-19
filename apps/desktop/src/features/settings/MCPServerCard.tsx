import { Trash2 } from 'lucide-react'

import type { McpServerSummary } from '@/shared/tauri/commands'
import { Button } from '@/shared/ui/button'

interface MCPServerCardProps {
  onDelete: (id: string) => void
  server: McpServerSummary
}

export function MCPServerCard({ onDelete, server }: MCPServerCardProps) {
  return (
    <article
      aria-label={server.displayName}
      className="rounded-md border border-border bg-surface p-4"
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h3 className="truncate font-medium text-sm">{server.displayName}</h3>
          <div className="mt-1 flex flex-wrap gap-2 text-muted-foreground text-xs">
            <span>{server.status}</span>
            <span>{server.origin}</span>
            <span>{server.scope}</span>
            <span>{server.transport}</span>
            <span>
              {server.exposedToolCount} {server.exposedToolCount === 1 ? 'tool' : 'tools'}
            </span>
          </div>
        </div>

        <div className="flex shrink-0 items-center gap-1">
          <Button
            aria-label={`Delete ${server.displayName}`}
            onClick={() => onDelete(server.id)}
            size="icon"
            type="button"
            variant="ghost"
          >
            <Trash2 className="size-4" />
          </Button>
        </div>
      </div>

      {server.lastError ? (
        <div className="mt-3 rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-xs">
          {server.lastError}
        </div>
      ) : null}
    </article>
  )
}
