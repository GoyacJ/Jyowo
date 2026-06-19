import {
  ArtifactHistory,
  type ArtifactHistoryItem,
  type ArtifactStatus,
} from '@/features/artifacts/ArtifactHistory'
import { ArtifactPreview, type ArtifactPreviewState } from '@/features/artifacts/ArtifactPreview'

interface ArtifactSummaryItem extends ArtifactHistoryItem {
  errorMessage?: string
  previewState?: ArtifactPreviewState
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
      <ArtifactHistory
        activeArtifactId={activeArtifactId}
        artifacts={artifacts}
        onOpenArtifact={onOpenArtifact}
        onOpenSource={onOpenSource}
        variant="compact"
      />
      {activeArtifact ? (
        <ArtifactPreview
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

function getDefaultPreviewState(status: ArtifactStatus): ArtifactPreviewState {
  if (status === 'failed') {
    return 'error'
  }

  return status === 'ready' ? 'ready' : 'loading'
}
