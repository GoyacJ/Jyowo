import { useQuery } from '@tanstack/react-query'
import { ExternalLink } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useUiStore } from '@/shared/state/ui-store'
import type { ArtifactSegment } from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'

export function ArtifactSegmentView({
  conversationId,
  revisionIdOverride,
  segment,
}: {
  conversationId: string
  revisionIdOverride?: string | null
  segment: ArtifactSegment
}) {
  const { t } = useTranslation('conversation')
  const media = segment.revision.media
  const shouldLoadImagePreview = segment.status === 'ready' && media?.kind === 'image'
  const revisionId =
    revisionIdOverride === null ? undefined : (revisionIdOverride ?? segment.revision.revisionId)
  const setSelection = useUiStore((state) => state.setWorkbenchSelection)
  const setInspectorOpen = useUiStore((state) => state.setInspectorOpen)

  return (
    <section className="rounded-md border border-border px-3 py-2">
      <div className="flex items-center justify-between">
        <div className="font-medium text-sm">{segment.title}</div>
        <button
          aria-label="Open artifact in inspector"
          className="inline-flex items-center gap-1 rounded px-2 py-1 text-muted-foreground text-xs hover:bg-muted hover:text-foreground"
          onClick={() => {
            setSelection({
              kind: 'artifact',
              conversationId,
              artifactId: segment.artifactId,
              ...(revisionId ? { revisionId } : {}),
            })
            setInspectorOpen(true)
          }}
          type="button"
        >
          <ExternalLink className="size-3" />
          {t('timeline.openInInspector')}
        </button>
      </div>
      {segment.summary ? (
        <p className="mt-1 text-muted-foreground text-sm">{segment.summary}</p>
      ) : null}
      {revisionId ? (
        <p className="mt-0.5 text-muted-foreground text-xs">
          {t('timeline.artifactRevision', { revisionId })}
        </p>
      ) : null}
      {shouldLoadImagePreview ? (
        <ArtifactImagePreview
          artifactId={segment.artifactId}
          conversationId={conversationId}
          revisionId={revisionId}
          title={segment.title}
        />
      ) : null}
    </section>
  )
}

function ArtifactImagePreview({
  artifactId,
  conversationId,
  revisionId,
  title,
}: {
  artifactId: string
  conversationId: string
  revisionId?: string
  title: string
}) {
  const { t } = useTranslation('conversation')
  const commandClient = useCommandClient()
  const previewQuery = useQuery({
    queryKey: ['conversation-artifact-preview', conversationId, artifactId, revisionId],
    queryFn: () =>
      commandClient.getArtifactMediaPreview({
        conversationId,
        artifactId,
        revisionId,
      }),
  })

  if (previewQuery.isPending) {
    return (
      <div className="mt-3 rounded-md border border-border bg-muted/40 px-3 py-8 text-center text-muted-foreground text-sm">
        {t('timeline.generatedArtifact')}
      </div>
    )
  }

  if (previewQuery.isError) {
    return (
      <div className="mt-3 rounded-md border border-border bg-muted/40 px-3 py-8 text-center text-muted-foreground text-sm">
        {t('timeline.imagePreviewUnavailable')}
      </div>
    )
  }

  return (
    <img
      alt={title}
      className="mt-3 max-h-[420px] w-full rounded-md border border-border object-contain"
      src={previewQuery.data.dataUrl}
    />
  )
}
