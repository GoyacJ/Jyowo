import { useQuery } from '@tanstack/react-query'
import { Download, ImageIcon } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { ArtifactPreview } from '@/shared/artifacts/ArtifactPreview'
import type {
  ArtifactListRevision,
  ArtifactSegment,
  ListArtifactsResponse,
} from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'
import { Button } from '@/shared/ui/button'

type ArtifactSummary = ListArtifactsResponse['artifacts'][number]

export function ArtifactPane({
  conversationId,
  initialRevisionId,
  segment,
}: {
  conversationId: string
  initialRevisionId?: string
  segment: ArtifactSegment
}) {
  const { t } = useTranslation('conversation')
  const commandClient = useCommandClient()
  const [selectedArtifactId, setSelectedArtifactId] = useState(segment.artifactId)
  const [selectedRevisionId, setSelectedRevisionId] = useState(
    initialRevisionId ?? segment.revision.revisionId,
  )
  const [exportedPath, setExportedPath] = useState<string | null>(null)

  useEffect(() => {
    setSelectedArtifactId(segment.artifactId)
    setSelectedRevisionId(initialRevisionId ?? segment.revision.revisionId)
    setExportedPath(null)
  }, [initialRevisionId, segment.artifactId, segment.revision.revisionId])

  const artifactsQuery = useQuery({
    queryKey: ['workbench-artifacts', conversationId],
    queryFn: () => commandClient.listArtifacts({ conversationId }),
  })

  const artifacts = useMemo(
    () => mergeSegmentArtifact(artifactsQuery.data?.artifacts ?? [], segment),
    [artifactsQuery.data?.artifacts, segment],
  )
  const selectedArtifact =
    artifacts.find((artifact) => artifact.id === selectedArtifactId) ?? artifacts[0]
  const revisions = useMemo(
    () => sortRevisionsNewestFirst(selectedArtifact?.revisions ?? []),
    [selectedArtifact?.revisions],
  )
  const matchedRevision = revisions.find((revision) => revision.revisionId === selectedRevisionId)
  const waitingForInitialRevision =
    Boolean(initialRevisionId) &&
    selectedRevisionId === initialRevisionId &&
    selectedArtifact?.id === segment.artifactId &&
    !matchedRevision
  const selectedRevision = matchedRevision ?? (waitingForInitialRevision ? undefined : revisions[0])
  const contentRef = selectedRevision?.contentRef
  const revisionStatus = selectedRevision?.status ?? selectedArtifact?.status ?? segment.status
  const revisionKind = selectedRevision?.kind ?? selectedArtifact?.kind ?? segment.revision.kind
  const isImage = selectedRevision?.media?.kind === 'image' || revisionKind === 'image'
  const canLoadContent =
    Boolean(contentRef) && revisionStatus !== 'failed' && !isImage && !selectedRevision?.media

  const contentQuery = useQuery({
    enabled: canLoadContent,
    queryKey: ['workbench-artifact-revision-content', conversationId, contentRef],
    queryFn: () =>
      commandClient.getArtifactRevisionContent({
        conversationId,
        contentRef: contentRef ?? '',
      }),
  })
  const imageQuery = useQuery({
    enabled: isImage && revisionStatus === 'ready' && Boolean(selectedRevision),
    queryKey: [
      'workbench-artifact-media-preview',
      conversationId,
      selectedArtifact?.id,
      selectedRevision?.revisionId,
    ],
    queryFn: () =>
      commandClient.getArtifactMediaPreview({
        conversationId,
        artifactId: selectedArtifact?.id ?? '',
        revisionId: selectedRevision?.revisionId,
      }),
  })

  const exportContent = async () => {
    if (!contentRef) {
      return
    }
    const result = await commandClient.exportConversationEvidence({
      conversationId,
      kind: 'artifact-content',
      refId: contentRef,
    })
    setExportedPath(result.path)
  }

  if (!selectedArtifact || !selectedRevision) {
    return (
      <PaneState
        description={t(
          'inspector.artifactNoContentDescription',
          'This artifact revision does not expose a content reference.',
        )}
        title={t('inspector.artifactNoContent', 'Artifact content unavailable')}
      />
    )
  }

  return (
    <div className="grid gap-3 p-3">
      <section className="grid gap-2 rounded-md border border-border px-3 py-2">
        <div className="min-w-0">
          <div className="truncate font-medium text-sm">{selectedArtifact.title}</div>
          <p className="text-muted-foreground text-xs">
            {revisionKind} · {revisionStatus} · {selectedRevision.revisionId}
          </p>
        </div>
        {selectedArtifact.description ? (
          <p className="text-muted-foreground text-sm">{selectedArtifact.description}</p>
        ) : null}
      </section>

      <section aria-label="Artifact list" className="grid gap-1">
        {artifacts.map((artifact) => (
          <Button
            className="justify-start"
            key={artifact.id}
            onClick={() => {
              const nextRevision = sortRevisionsNewestFirst(artifact.revisions ?? [])[0]
              setSelectedArtifactId(artifact.id)
              setSelectedRevisionId(nextRevision?.revisionId ?? '')
              setExportedPath(null)
            }}
            type="button"
            variant={artifact.id === selectedArtifact.id ? 'secondary' : 'ghost'}
          >
            {artifact.title}
          </Button>
        ))}
      </section>

      <section aria-label="Artifact revisions" className="grid gap-1">
        {revisions.map((revision) => (
          <Button
            className="justify-start"
            key={revision.revisionId}
            onClick={() => {
              setSelectedRevisionId(revision.revisionId)
              setExportedPath(null)
            }}
            type="button"
            variant={revision.revisionId === selectedRevision.revisionId ? 'secondary' : 'ghost'}
          >
            {revision.revisionId} · {revision.status ?? selectedArtifact.status}
          </Button>
        ))}
      </section>

      {revisionStatus === 'failed' ? (
        <section className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
          {t('inspector.artifactRevisionFailed', 'Artifact revision failed')}
        </section>
      ) : null}

      {contentRef ? (
        <div className="flex items-center justify-between gap-2">
          <Button onClick={exportContent} size="sm" type="button" variant="outline">
            <Download className="size-4" />
            {t('inspector.exportArtifactContent', 'Export content')}
          </Button>
          {exportedPath ? (
            <span className="truncate text-muted-foreground text-xs">{exportedPath}</span>
          ) : null}
        </div>
      ) : null}

      <ArtifactRevisionPreview
        content={contentQuery.data?.content}
        contentType={contentQuery.data?.contentType}
        error={contentQuery.isError || imageQuery.isError}
        imageDataUrl={imageQuery.data?.dataUrl}
        isImage={isImage}
        kind={revisionKind}
        loading={contentQuery.isLoading || imageQuery.isLoading}
        status={revisionStatus}
        title={selectedRevision.title ?? selectedArtifact.title}
        truncated={contentQuery.data?.truncated}
      />
      {contentQuery.data?.truncated ? (
        <div className="rounded-md border border-border px-3 py-2 text-muted-foreground text-xs">
          {t('inspector.artifactContentTruncated', 'Artifact content page truncated')}
        </div>
      ) : null}
    </div>
  )
}

function ArtifactRevisionPreview({
  content,
  contentType,
  error,
  imageDataUrl,
  isImage,
  kind,
  loading,
  status,
  title,
  truncated,
}: {
  content?: string
  contentType?: string
  error: boolean
  imageDataUrl?: string
  isImage: boolean
  kind?: string
  loading: boolean
  status?: string
  title: string
  truncated?: boolean
}) {
  const { t } = useTranslation('conversation')

  if (status === 'failed') {
    return (
      <ArtifactPreview
        errorMessage={t('inspector.artifactRevisionFailed', 'Artifact revision failed')}
        kind={kind}
        state="error"
        title={title}
      />
    )
  }

  if (isImage) {
    return imageDataUrl || loading || error ? (
      <ArtifactPreview
        errorMessage={t(
          'inspector.artifactErrorDescription',
          'The artifact content could not be loaded.',
        )}
        imageDataUrl={imageDataUrl}
        kind={kind}
        state={error ? 'error' : loading ? 'loading' : 'ready'}
        title={title}
      />
    ) : (
      <section className="rounded-md border border-border px-3 py-2 text-muted-foreground text-sm">
        <span className="flex items-center gap-2">
          <ImageIcon className="size-4" />
          {t('inspector.artifactMediaOnly', 'Artifact media preview unavailable')}
        </span>
      </section>
    )
  }

  if (loading || error || content !== undefined) {
    return (
      <ArtifactPreview
        content={content}
        contentType={contentType}
        errorMessage={t(
          'inspector.artifactErrorDescription',
          'The artifact content could not be loaded.',
        )}
        kind={kind}
        state={error ? 'error' : loading ? 'loading' : 'ready'}
        title={title}
        truncated={truncated}
      />
    )
  }

  return (
    <PaneState
      description={t(
        'inspector.artifactNoContentDescription',
        'This artifact revision does not expose a content reference.',
      )}
      title={t('inspector.artifactNoContent', 'Artifact content unavailable')}
    />
  )
}

function mergeSegmentArtifact(
  artifacts: readonly ArtifactSummary[],
  segment: ArtifactSegment,
): ArtifactSummary[] {
  const fallback = artifactFromSegment(segment)
  const exists = artifacts.some((artifact) => artifact.id === segment.artifactId)
  return exists ? [...artifacts] : [fallback, ...artifacts]
}

function artifactFromSegment(segment: ArtifactSegment): ArtifactSummary {
  return {
    actionLabel: 'Open',
    description: segment.summary ?? segment.title,
    id: segment.artifactId,
    kind: segment.artifactKind ?? segment.revision.kind,
    preview: segment.summary,
    revisions: [revisionFromSegment(segment)],
    status: segment.status ?? segment.revision.status,
    title: segment.title,
    updatedAt: new Date(0).toISOString(),
  }
}

function revisionFromSegment(segment: ArtifactSegment): ArtifactListRevision {
  return {
    contentRef: segment.revision.contentRef,
    kind: segment.revision.kind,
    media: segment.revision.media,
    previewRef: segment.revision.previewRef,
    revisionId: segment.revision.revisionId,
    status: segment.revision.status,
    summary: segment.revision.summary,
    title: segment.revision.title,
    updatedAt: new Date(0).toISOString(),
  }
}

function sortRevisionsNewestFirst(revisions: readonly ArtifactListRevision[]) {
  return [...revisions].sort((left, right) => {
    const rightTime = Date.parse(right.updatedAt)
    const leftTime = Date.parse(left.updatedAt)
    return (Number.isNaN(rightTime) ? 0 : rightTime) - (Number.isNaN(leftTime) ? 0 : leftTime)
  })
}

function PaneState({ title, description }: { title: string; description: string }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 px-6 text-center">
      <h3 className="text-sm font-medium text-foreground">{title}</h3>
      <p className="text-xs text-muted-foreground">{description}</p>
    </div>
  )
}
