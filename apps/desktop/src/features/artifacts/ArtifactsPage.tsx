import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ArtifactPreview } from '@/shared/artifacts/ArtifactPreview'
import type { ListArtifactsResponse } from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'
import { ArtifactHistory } from './ArtifactHistory'

const artifactsQueryKey = ['artifacts'] as const
const artifactContentMissingError = 'artifact content reference missing'

type ArtifactSummary = ListArtifactsResponse['artifacts'][number]
type ArtifactRevision = NonNullable<ArtifactSummary['revisions']>[number]

function sortArtifactsByUpdatedAt(artifacts: readonly ArtifactSummary[]): ArtifactSummary[] {
  return [...artifacts].sort((left, right) => {
    const rightUpdatedAt = Date.parse(right.updatedAt ?? latestRevision(right)?.updatedAt ?? '')
    const leftUpdatedAt = Date.parse(left.updatedAt ?? latestRevision(left)?.updatedAt ?? '')

    return (
      (Number.isNaN(rightUpdatedAt) ? 0 : rightUpdatedAt) -
      (Number.isNaN(leftUpdatedAt) ? 0 : leftUpdatedAt)
    )
  })
}

function sortedRevisions(artifact: ArtifactSummary): ArtifactRevision[] {
  return [...(artifact.revisions ?? [])].sort((left, right) => {
    const rightUpdatedAt = Date.parse(right.updatedAt)
    const leftUpdatedAt = Date.parse(left.updatedAt)

    return (
      (Number.isNaN(rightUpdatedAt) ? 0 : rightUpdatedAt) -
      (Number.isNaN(leftUpdatedAt) ? 0 : leftUpdatedAt)
    )
  })
}

function latestRevision(artifact: ArtifactSummary): ArtifactRevision | undefined {
  return sortedRevisions(artifact)[0]
}

function isImageArtifact(kind: string | undefined): boolean {
  return kind === 'image' || kind?.startsWith('image/') === true
}

export function ArtifactsPage() {
  const { t } = useTranslation('artifacts')
  const commandClient = useCommandClient()
  const conversationsQuery = useQuery({
    queryFn: () => commandClient.listConversations(),
    queryKey: ['artifacts-conversations'],
  })
  const conversationId = conversationsQuery.data?.conversations[0]?.id
  const artifactsQuery = useQuery({
    enabled: Boolean(conversationId),
    queryFn: () => {
      if (!conversationId) {
        throw new Error('conversationId is required for artifact listing')
      }

      return commandClient.listArtifacts({ conversationId })
    },
    queryKey: [...artifactsQueryKey, conversationId],
  })
  const isError = conversationsQuery.isError || artifactsQuery.isError
  const isLoading =
    conversationsQuery.isLoading || (Boolean(conversationId) && artifactsQuery.isLoading)
  const artifacts = isError ? [] : sortArtifactsByUpdatedAt(artifactsQuery.data?.artifacts ?? [])
  const [activeArtifactId, setActiveArtifactId] = useState<string | undefined>()
  const activeArtifact =
    artifacts.find((artifact) => artifact.id === activeArtifactId) ?? artifacts[0]
  const previewState = isError
    ? 'error'
    : isLoading
      ? 'loading'
      : activeArtifact
        ? 'ready'
        : 'error'

  return (
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-5">
      <header>
        <h1 className="font-semibold text-2xl">{t('pageTitle')}</h1>
        <p className="mt-1 text-muted-foreground text-sm">{t('pageDescription')}</p>
      </header>

      <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_360px]">
        {isError ? (
          <section
            aria-label={t('history')}
            className="rounded-md border border-destructive/30 bg-destructive/5 px-4 py-3"
          >
            <h2 className="font-medium text-destructive text-sm">{t('historyLoadError')}</h2>
          </section>
        ) : isLoading ? (
          <section
            aria-label={t('history')}
            className="rounded-md border border-border bg-surface px-4 py-3 text-muted-foreground text-sm"
          >
            {t('loading')}
          </section>
        ) : (
          <ArtifactHistory
            activeArtifactId={activeArtifact?.id}
            artifacts={artifacts}
            onOpenArtifact={setActiveArtifactId}
          />
        )}
        <ArtifactPreviewLoader
          artifact={activeArtifact}
          conversationId={conversationId}
          errorMessage={isError ? t('previewUnavailable') : t('noArtifactSelected')}
          state={previewState}
        />
      </div>
    </div>
  )
}

interface ArtifactPreviewLoaderProps {
  artifact?: ArtifactSummary
  conversationId?: string
  errorMessage: string
  state: 'error' | 'loading' | 'ready'
}

function ArtifactPreviewLoader({
  artifact,
  conversationId,
  errorMessage,
  state,
}: ArtifactPreviewLoaderProps) {
  const { t } = useTranslation('artifacts')
  const commandClient = useCommandClient()
  const revision = artifact ? latestRevision(artifact) : undefined
  const isImage = isImageArtifact(artifact?.kind)
  const contentRef = revision?.contentRef
  const usesSummaryPreview = state === 'ready' && Boolean(artifact) && !isImage && !contentRef
  const contentQuery = useQuery({
    enabled:
      state === 'ready' &&
      Boolean(conversationId) &&
      Boolean(artifact) &&
      !isImage &&
      Boolean(contentRef),
    queryFn: () => {
      if (!conversationId || !contentRef) {
        throw new Error(artifactContentMissingError)
      }

      return commandClient.getArtifactRevisionContent({
        conversationId,
        contentRef,
      })
    },
    queryKey: [
      'artifact-revision-content',
      conversationId,
      artifact?.id,
      revision?.revisionId,
      contentRef,
    ],
  })
  const imageQuery = useQuery({
    enabled: state === 'ready' && Boolean(conversationId) && Boolean(artifact) && isImage,
    queryFn: () => {
      if (!conversationId || !artifact) {
        throw new Error('artifact image preview context missing')
      }

      return commandClient.getArtifactMediaPreview({
        artifactId: artifact.id,
        conversationId,
        revisionId: revision?.revisionId,
      })
    },
    queryKey: ['artifact-media-preview', conversationId, artifact?.id, revision?.revisionId],
  })

  if (state !== 'ready') {
    return (
      <ArtifactPreview
        errorMessage={errorMessage}
        kind={artifact?.kind}
        state={state}
        title={artifact?.title ?? t('pageTitle')}
      />
    )
  }

  if (!artifact) {
    return <ArtifactPreview errorMessage={errorMessage} state="error" title={t('pageTitle')} />
  }

  if (isImage) {
    return (
      <ArtifactPreview
        errorMessage={t('previewUnavailable')}
        imageDataUrl={imageQuery.data?.dataUrl}
        kind={artifact.kind}
        state={imageQuery.isError ? 'error' : imageQuery.isLoading ? 'loading' : 'ready'}
        title={artifact.title}
      />
    )
  }

  if (usesSummaryPreview) {
    return (
      <ArtifactPreview
        content={artifact.preview}
        errorMessage={t('missingContentRef')}
        kind={artifact.kind}
        state="ready"
        title={artifact.title}
      />
    )
  }

  return (
    <ArtifactPreview
      content={contentQuery.data?.content}
      contentType={contentQuery.data?.contentType}
      errorMessage={t('previewUnavailable')}
      kind={artifact.kind}
      state={contentQuery.isError ? 'error' : contentQuery.isLoading ? 'loading' : 'ready'}
      title={artifact.title}
      truncated={contentQuery.data?.truncated}
    />
  )
}
