import {
  AlertCircle,
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
type ArtifactPreviewState = 'error' | 'loading' | 'ready'

interface ArtifactSummaryItem {
  actionLabel: string
  description: string
  errorMessage?: string
  id: string
  kind: string
  preview?: string
  previewState?: ArtifactPreviewState
  sourceMessageId?: string
  sourceRunId: string
  status: ArtifactStatus
  title: string
}

export interface ArtifactSummaryProps {
  activeArtifactId?: string
  artifacts: readonly ArtifactSummaryItem[]
  onOpenArtifact?: (artifactId: string) => void
  onOpenSource?: (messageId: string) => void
}

export function ArtifactSummary({
  activeArtifactId,
  artifacts,
  onOpenArtifact,
  onOpenSource,
}: ArtifactSummaryProps) {
  const activeArtifact = activeArtifactId
    ? (artifacts.find((artifact) => artifact.id === activeArtifactId) ?? null)
    : null

  return (
    <section className="mt-3">
      <CompactArtifactHistory
        activeArtifactId={activeArtifactId}
        artifacts={artifacts}
        onOpenArtifact={onOpenArtifact}
        onOpenSource={onOpenSource}
      />
      {activeArtifact ? (
        <CompactArtifactPreview
          content={activeArtifact.preview}
          errorMessage={activeArtifact.errorMessage}
          kind={activeArtifact.kind}
          state={activeArtifact.previewState ?? getDefaultPreviewState(activeArtifact.status)}
          title={activeArtifact.title}
        />
      ) : null}
    </section>
  )
}

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

function CompactArtifactHistory({
  activeArtifactId,
  artifacts,
  onOpenArtifact,
  onOpenSource,
}: {
  activeArtifactId?: string
  artifacts: readonly ArtifactSummaryItem[]
  onOpenArtifact?: (artifactId: string) => void
  onOpenSource?: (messageId: string) => void
}) {
  return (
    <section aria-label="Artifact history" className="space-y-3">
      {artifacts.map((artifact) => {
        const StatusIcon = statusIcons[artifact.status]
        const isActive = artifact.id === activeArtifactId

        return (
          <article
            aria-current={isActive ? 'true' : undefined}
            aria-label={artifact.title}
            className={cn(
              'rounded-md border bg-surface px-3 py-1.5',
              isActive ? 'border-primary/50' : 'border-border',
            )}
            key={artifact.id}
          >
            <div className="flex items-start justify-between gap-4">
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2">
                  <span
                    className={cn(
                      'grid size-6 shrink-0 place-items-center rounded-full border',
                      statusStyles[artifact.status],
                    )}
                  >
                    <StatusIcon
                      className={cn(
                        'size-3.5',
                        artifact.status === 'running' ? 'animate-spin' : '',
                      )}
                    />
                  </span>
                  <h3 className="font-medium text-sm">{artifact.title}</h3>
                </div>
                <p className="mt-0.5 text-muted-foreground text-xs leading-4">
                  {artifact.description}
                </p>
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

function CompactArtifactPreview({
  content = '',
  errorMessage = 'Artifact preview unavailable.',
  kind = 'artifact',
  state,
  title,
}: {
  content?: string
  errorMessage?: string
  kind?: string
  state: ArtifactPreviewState
  title: string
}) {
  if (state === 'loading') {
    return (
      <section
        aria-label={`${title} preview`}
        className="mt-3 rounded-md border border-border bg-background px-3 py-3 text-muted-foreground text-sm"
      >
        <span className="flex items-center gap-2">
          <Loader2 className="size-4 animate-spin" />
          Loading artifact preview.
        </span>
      </section>
    )
  }

  if (state === 'error') {
    return (
      <section
        aria-label={`${title} preview`}
        className="mt-3 rounded-md border border-destructive/30 bg-destructive/5 px-3 py-3 text-destructive text-sm"
      >
        <span className="flex items-center gap-2">
          <AlertCircle className="size-4" />
          {errorMessage}
        </span>
      </section>
    )
  }

  return (
    <section
      aria-label={`${title} preview`}
      className="mt-3 rounded-md border border-border bg-background"
    >
      <div className="flex items-center justify-between border-border border-b px-3 py-2">
        <span className="flex items-center gap-2 font-medium text-sm">
          <FileText className="size-4 text-muted-foreground" />
          Preview
        </span>
        <span className="text-muted-foreground text-xs">{kind}</span>
      </div>
      <pre className="max-h-64 overflow-auto whitespace-pre-wrap px-3 py-3 text-sm">
        {content || 'No preview available.'}
      </pre>
    </section>
  )
}

function getDefaultPreviewState(status: ArtifactStatus): ArtifactPreviewState {
  if (status === 'failed') {
    return 'error'
  }

  return status === 'ready' ? 'ready' : 'loading'
}
