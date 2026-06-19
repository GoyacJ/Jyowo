import {
  Check,
  Clock,
  ExternalLink,
  FileText,
  Loader2,
  type LucideIcon,
  TriangleAlert,
} from 'lucide-react'

import { cn } from '@/shared/lib/utils'
import { Button } from '@/shared/ui/button'

type ArtifactStatus = 'failed' | 'pending' | 'ready' | 'running'

interface ArtifactHistoryItem {
  actionLabel: string
  description: string
  id: string
  kind: string
  preview?: string
  sourceMessageId?: string
  sourceRunId: string
  status: ArtifactStatus
  title: string
}

interface ArtifactHistoryProps {
  activeArtifactId?: string
  artifacts: readonly ArtifactHistoryItem[]
  onOpenArtifact?: (artifactId: string) => void
  onOpenSource?: (messageId: string) => void
  variant?: 'compact' | 'default'
}

const statusLabels = {
  failed: 'Failed',
  pending: 'Pending',
  ready: 'Ready',
  running: 'Running',
} satisfies Record<ArtifactStatus, string>

const statusIcons = {
  failed: TriangleAlert,
  pending: Clock,
  ready: Check,
  running: Loader2,
} satisfies Record<ArtifactStatus, LucideIcon>

const statusStyles = {
  failed: 'border-destructive/40 text-destructive',
  pending: 'border-muted-foreground/40 text-muted-foreground',
  ready: 'border-success text-success',
  running: 'border-primary/40 text-primary',
} satisfies Record<ArtifactStatus, string>

export function ArtifactHistory({
  activeArtifactId,
  artifacts,
  onOpenArtifact,
  onOpenSource,
  variant = 'default',
}: ArtifactHistoryProps) {
  if (artifacts.length === 0) {
    return (
      <section className="rounded-md border border-border bg-surface px-4 py-3">
        <h3 className="font-medium text-sm">No artifacts for this conversation.</h3>
        <p className="mt-1 text-muted-foreground text-sm">
          Artifacts appear after Jyowo produces reviewable work.
        </p>
      </section>
    )
  }

  return (
    <section aria-label="Artifact history" className="space-y-3">
      {artifacts.map((artifact) => {
        const StatusIcon = statusIcons[artifact.status]
        const isActive = artifact.id === activeArtifactId
        const compact = variant === 'compact'

        return (
          <article
            aria-label={artifact.title}
            aria-current={isActive ? 'true' : undefined}
            className={cn(
              'rounded-md border bg-surface',
              compact ? 'px-3 py-1.5' : 'px-4 py-3',
              isActive ? 'border-primary/50' : 'border-border',
            )}
            key={artifact.id}
          >
            <div className="flex items-start justify-between gap-4">
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2">
                  <span
                    className={cn(
                      'grid shrink-0 place-items-center rounded-full border',
                      compact ? 'size-6' : 'size-8',
                      statusStyles[artifact.status],
                    )}
                  >
                    <StatusIcon
                      className={cn(
                        compact ? 'size-3.5' : 'size-4',
                        artifact.status === 'running' ? 'animate-spin' : '',
                      )}
                    />
                  </span>
                  <h3 className="font-medium text-sm">{artifact.title}</h3>
                  {compact ? null : (
                    <>
                      <span className="rounded-md border border-border px-2 py-0.5 text-muted-foreground text-xs">
                        {artifact.kind}
                      </span>
                      <span className="rounded-md border border-border px-2 py-0.5 text-xs">
                        {statusLabels[artifact.status]}
                      </span>
                    </>
                  )}
                </div>
                <p
                  className={cn(
                    'text-muted-foreground',
                    compact ? 'mt-0.5 text-xs leading-4' : 'mt-2 text-sm',
                  )}
                >
                  {artifact.description}
                </p>
                {compact ? null : (
                  <div className="mt-2 flex flex-wrap items-center gap-3 text-muted-foreground text-xs">
                    <span className="inline-flex items-center gap-1">
                      <FileText className="size-3.5" />
                      Source run
                    </span>
                    <span>{artifact.sourceRunId}</span>
                  </div>
                )}
              </div>

              <div className="flex shrink-0 items-center gap-2">
                <Button
                  size="sm"
                  type="button"
                  variant="outline"
                  onClick={() => onOpenArtifact?.(artifact.id)}
                >
                  <ExternalLink className="size-4" />
                  {artifact.actionLabel}
                </Button>
                {artifact.sourceMessageId ? (
                  <Button
                    aria-label="Show source message"
                    size="icon"
                    type="button"
                    variant="outline"
                    onClick={() => onOpenSource?.(artifact.sourceMessageId ?? '')}
                  >
                    <FileText className="size-4" />
                  </Button>
                ) : null}
              </div>
            </div>
          </article>
        )
      })}
    </section>
  )
}
