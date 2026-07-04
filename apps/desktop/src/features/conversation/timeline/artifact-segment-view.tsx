import { useQuery } from '@tanstack/react-query'
import { ExternalLink } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { ArtifactSegment } from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'
import { useUiStore } from '@/shared/state/ui-store'

export function ArtifactSegmentView({
  conversationId,
  segment,
}: {
  conversationId: string
  segment: ArtifactSegment
}) {
  const { t } = useTranslation('conversation')
  const media = segment.revision.media
  const shouldLoadImagePreview = segment.status === 'ready' && media?.kind === 'image'
  const setSelection = useUiStore((state) => state.setWorkbenchSelection)
  const setInspectorOpen = useUiStore((state) => state.setInspectorOpen)

  return (
    <section className="rounded-md border border-border px-3 py-2">
      <div className="flex items-center justify-between">
        <div className="font-medium text-sm">{segment.title}</div>
        <button
          className="inline-flex items-center gap-1 rounded px-2 py-1 text-muted-foreground text-xs hover:bg-muted hover:text-foreground"
          onClick={() => {
            setSelection({
              kind: 'artifact',
              conversationId,
              artifactId: segment.artifactId,
              revisionId: segment.revision.revisionId,
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
      <p className="mt-0.5 text-muted-foreground text-xs">
        {t('timeline.artifactRevision', { revisionId: segment.revision.revisionId })}
      </p>
      {shouldLoadImagePreview ? (
        <ArtifactImagePreview
          artifactId={segment.artifactId}
          conversationId={conversationId}
          title={segment.title}
        />
      ) : null}
    </section>
  )
}

export function ArtifactImagePreview({
  artifactId,
  conversationId,
  title,
}: {
  artifactId: string
  conversationId: string
  title: string
}) {
  const { t } = useTranslation('conversation')
  const commandClient = useCommandClient()
  const previewQuery = useQuery({
    queryKey: ['conversation-artifact-preview', conversationId, artifactId],
    queryFn: () =>
      commandClient.getArtifactMediaPreview({
        conversationId,
        artifactId,
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
