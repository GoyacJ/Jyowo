import { AlertCircle, FileText, Loader2 } from 'lucide-react'

export type ArtifactPreviewState = 'error' | 'loading' | 'ready'

export interface ArtifactPreviewProps {
  content?: string
  errorMessage?: string
  kind?: string
  maxPreviewCharacters?: number
  state: ArtifactPreviewState
  title: string
}

const defaultMaxPreviewCharacters = 1600

export function ArtifactPreview({
  content = '',
  errorMessage = 'Artifact preview unavailable.',
  kind = 'artifact',
  maxPreviewCharacters = defaultMaxPreviewCharacters,
  state,
  title,
}: ArtifactPreviewProps) {
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

  const hasLargePreview = content.length > maxPreviewCharacters
  const previewContent = hasLargePreview ? content.slice(0, maxPreviewCharacters) : content

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
        {previewContent || 'No preview available.'}
      </pre>
      {hasLargePreview ? (
        <div className="border-border border-t px-3 py-2 text-muted-foreground text-xs">
          <span className="block font-medium text-foreground">Large preview truncated.</span>
          <span>Open artifact to inspect the full output.</span>
        </div>
      ) : null}
    </section>
  )
}
