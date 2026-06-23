import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useCommandClient } from '@/shared/tauri/react'

import { ArtifactHistory } from './ArtifactHistory'
import { ArtifactPreview } from './ArtifactPreview'

const artifactsQueryKey = ['artifacts'] as const

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
  const artifacts = isError ? [] : (artifactsQuery.data?.artifacts ?? [])
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
        <ArtifactPreview
          content={activeArtifact?.preview}
          errorMessage={isError ? t('previewUnavailable') : t('noArtifactSelected')}
          kind={activeArtifact?.kind}
          state={previewState}
          title={activeArtifact?.title ?? t('pageTitle')}
        />
      </div>
    </div>
  )
}
