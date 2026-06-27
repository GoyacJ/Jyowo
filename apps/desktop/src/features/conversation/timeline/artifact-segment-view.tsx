import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import type { ArtifactSegment } from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'

export function ArtifactSegmentView({
  conversationId,
  segment,
}: {
  conversationId: string
  segment: ArtifactSegment
}) {
  const shouldLoadImagePreview = segment.status === 'ready' && segment.media?.kind === 'image'

  return (
    <section className="rounded-md border border-border px-3 py-2">
      <div className="font-medium text-sm">{segment.title}</div>
      {segment.summary ? (
        <p className="mt-1 text-muted-foreground text-sm">{segment.summary}</p>
      ) : null}
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
