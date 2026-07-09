import { AlertCircle, FileText, ImageIcon, Loader2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'

type ArtifactPreviewState = 'error' | 'loading' | 'ready'

export interface ArtifactPreviewProps {
  content?: string
  contentType?: string
  errorMessage?: string
  imageDataUrl?: string
  kind?: string
  maxPreviewCharacters?: number
  state: ArtifactPreviewState
  title: string
  truncated?: boolean
}

const defaultMaxPreviewCharacters = 1600
const htmlPreviewCsp =
  "default-src 'none'; img-src data: blob:; style-src 'unsafe-inline'; connect-src 'none'; frame-src 'none'; base-uri 'none'; form-action 'none'"

function sandboxedHtmlPreview(content: string) {
  return `<!doctype html><html><head><meta http-equiv="Content-Security-Policy" content="${htmlPreviewCsp}"></head><body>${content}</body></html>`
}

export function ArtifactPreview({
  content = '',
  contentType,
  errorMessage,
  imageDataUrl,
  kind = 'artifact',
  maxPreviewCharacters = defaultMaxPreviewCharacters,
  state,
  title,
  truncated = false,
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

  const hasLargePreview = truncated || content.length > maxPreviewCharacters
  const previewContent = hasLargePreview ? content.slice(0, maxPreviewCharacters) : content
  const isHtmlPreview = kind === 'html' || contentType?.split(';')[0]?.trim() === 'text/html'
  const isImagePreview = Boolean(imageDataUrl)

  return (
    <section
      aria-label={`${title} preview`}
      className="mt-3 rounded-md border border-border bg-background"
    >
      <div className="flex items-center justify-between border-border border-b px-3 py-2">
        <span className="flex items-center gap-2 font-medium text-sm">
          {isImagePreview ? (
            <ImageIcon className="size-4 text-muted-foreground" />
          ) : (
            <FileText className="size-4 text-muted-foreground" />
          )}
          {t('preview')}
        </span>
        <span className="text-muted-foreground text-xs">{kind}</span>
      </div>
      {imageDataUrl ? (
        <div className="grid aspect-video min-h-48 place-items-center bg-muted/20 px-3 py-3">
          <img
            alt={title}
            className="h-full max-h-96 max-w-full rounded-sm object-contain"
            height={384}
            src={imageDataUrl}
            width={682}
          />
        </div>
      ) : isHtmlPreview ? (
        <iframe
          className="h-96 w-full bg-background"
          referrerPolicy="no-referrer"
          sandbox=""
          srcDoc={sandboxedHtmlPreview(previewContent)}
          title={`${title} sandboxed preview`}
        />
      ) : (
        <pre className="max-h-64 overflow-auto whitespace-pre-wrap px-3 py-3 text-sm">
          {previewContent || t('noPreview')}
        </pre>
      )}
      {hasLargePreview ? (
        <div className="border-border border-t px-3 py-2 text-muted-foreground text-xs">
          <span className="block font-medium text-foreground">{t('largePreviewTruncated')}</span>
          <span>{t('openOutputPage')}</span>
        </div>
      ) : null}
    </section>
  )
}
