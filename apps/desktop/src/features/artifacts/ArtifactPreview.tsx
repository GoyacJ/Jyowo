import { AlertCircle, FileText, Loader2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'

type ArtifactPreviewState = 'error' | 'loading' | 'ready'

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
  errorMessage,
  kind = 'artifact',
  maxPreviewCharacters = defaultMaxPreviewCharacters,
  state,
  title,
}: ArtifactPreviewProps) {
  const { t } = useTranslation('artifacts')

  if (state === 'loading') {
    return (
      <section
        aria-label={`${title} preview`}
        className="mt-3 rounded-md border border-border bg-background px-3 py-3 text-muted-foreground text-sm"
      >
        <span className="flex items-center gap-2">
          <Loader2 className="size-4 animate-spin" />
          {t('loadingPreview')}
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
          {errorMessage ?? t('previewUnavailable')}
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
          {t('preview')}
        </span>
        <span className="text-muted-foreground text-xs">{kind}</span>
      </div>
      <pre className="max-h-64 overflow-auto whitespace-pre-wrap px-3 py-3 text-sm">
        {previewContent || t('noPreview')}
      </pre>
      {hasLargePreview ? (
        <div className="border-border border-t px-3 py-2 text-muted-foreground text-xs">
          <span className="block font-medium text-foreground">{t('largePreviewTruncated')}</span>
          <span>{t('openFullOutput')}</span>
        </div>
      ) : null}
    </section>
  )
}
