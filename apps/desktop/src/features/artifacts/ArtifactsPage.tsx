import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { useCommandClient } from '@/shared/tauri/react'

import { ArtifactHistory } from './ArtifactHistory'
import { ArtifactPreview } from './ArtifactPreview'

const artifactsQueryKey = ['artifacts'] as const

export function ArtifactsPage() {
  const commandClient = useCommandClient()
  const artifactsQuery = useQuery({
    queryFn: () => commandClient.listArtifacts(),
    queryKey: artifactsQueryKey,
  })
  const artifacts = artifactsQuery.isError ? [] : (artifactsQuery.data?.artifacts ?? [])
  const [activeArtifactId, setActiveArtifactId] = useState<string | undefined>()
  const activeArtifact =
    artifacts.find((artifact) => artifact.id === activeArtifactId) ?? artifacts[0]
  const previewState = artifactsQuery.isError
    ? 'error'
    : artifactsQuery.isLoading
      ? 'loading'
      : activeArtifact
        ? 'ready'
        : 'error'

  return (
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-5">
      <header>
        <h1 className="font-semibold text-2xl">Artifacts</h1>
        <p className="mt-1 text-muted-foreground text-sm">
          Review work products generated from conversation runs.
        </p>
      </header>

      <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_360px]">
        {artifactsQuery.isError ? (
          <section
            aria-label="Artifact history"
            className="rounded-md border border-destructive/30 bg-destructive/5 px-4 py-3"
          >
            <h2 className="font-medium text-destructive text-sm">
              Artifact history could not be loaded.
            </h2>
          </section>
        ) : artifactsQuery.isLoading ? (
          <section
            aria-label="Artifact history"
            className="rounded-md border border-border bg-surface px-4 py-3 text-muted-foreground text-sm"
          >
            Loading artifacts
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
          errorMessage={
            artifactsQuery.isError ? 'Artifact preview unavailable.' : 'No artifact selected.'
          }
          kind={activeArtifact?.kind}
          state={previewState}
          title={activeArtifact?.title ?? 'Artifacts'}
        />
      </div>
    </div>
  )
}
