import { AlertCircle, LoaderCircle } from 'lucide-react'
import { Component, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { type ArtifactDescriptor, type ArtifactSurface, normalizeArtifactDescriptor } from './model'
import { artifactRendererRegistry } from './renderers'
import { type ArtifactBlobLoader, useArtifactResource } from './resource'

export function ArtifactRenderer({
  artifact: sourceArtifact,
  loader,
  surface,
}: {
  artifact: ArtifactDescriptor
  loader?: ArtifactBlobLoader
  surface: ArtifactSurface
}) {
  const { t } = useTranslation('tasks')
  const artifact = normalizeArtifactDescriptor(sourceArtifact)
  const resource = useArtifactResource(artifact, loader, surface)
  const resolvedArtifact = {
    ...artifact,
    mediaType: resource.mediaType,
    size: resource.size ?? artifact.size,
  }
  const definition = artifactRendererRegistry.resolve(resolvedArtifact, surface)
  const View = definition?.views[surface]

  if (resource.loading) {
    return (
      <div
        className="flex min-h-24 items-center justify-center gap-2 text-muted-foreground text-sm"
        role="status"
      >
        <LoaderCircle aria-hidden="true" className="size-4 animate-spin" />
        {t('workbench.artifact.loading')}
      </div>
    )
  }
  if (resource.error) {
    return (
      <div
        className="flex min-h-28 flex-col items-center justify-center gap-3 text-center"
        role="alert"
      >
        <AlertCircle aria-hidden="true" className="size-5 text-destructive" />
        <p className="text-sm">{t('workbench.artifact.loadFailed')}</p>
        <button
          className="rounded-md border border-border px-2.5 py-1 text-xs hover:bg-muted"
          onClick={resource.retry}
          type="button"
        >
          {t('workbench.artifact.retry')}
        </button>
      </div>
    )
  }
  if (resource.missing) {
    return (
      <div
        className="flex min-h-28 flex-col items-center justify-center gap-2 text-center"
        role="status"
      >
        <AlertCircle aria-hidden="true" className="size-5 text-muted-foreground" />
        <p className="text-sm">{t('workbench.artifact.unavailable')}</p>
      </div>
    )
  }
  if (!View) return null
  return (
    <ArtifactRenderBoundary
      fallback={
        <div
          className="flex min-h-28 flex-col items-center justify-center gap-2 text-center"
          role="alert"
        >
          <AlertCircle aria-hidden="true" className="size-5 text-destructive" />
          <p className="text-sm">{t('workbench.artifact.renderFailed')}</p>
        </div>
      }
      key={`${definition.id}:${artifact.artifactId ?? artifact.blobId ?? artifact.title}:${surface}`}
    >
      <View artifact={resolvedArtifact} resource={resource} surface={surface} />
    </ArtifactRenderBoundary>
  )
}

class ArtifactRenderBoundary extends Component<
  { children: ReactNode; fallback: ReactNode },
  { failed: boolean }
> {
  state = { failed: false }

  static getDerivedStateFromError() {
    return { failed: true }
  }

  render() {
    return this.state.failed ? this.props.fallback : this.props.children
  }
}
