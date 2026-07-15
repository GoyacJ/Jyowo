import { useTranslation } from 'react-i18next'

import { Button } from '@/shared/ui/button'

export function DiffPanel({
  error,
  loading,
  missing,
  onRetry,
  text,
}: {
  error?: boolean
  loading: boolean
  missing: boolean
  onRetry?: () => void
  text: string | null
}) {
  const { t } = useTranslation('tasks')
  return (
    <ArtifactText
      empty={t('workbench.empty.changes')}
      error={error}
      loading={loading}
      missing={missing}
      onRetry={onRetry}
      text={text}
    />
  )
}

export function ArtifactText({
  empty,
  error = false,
  loading,
  missing,
  onRetry,
  text,
}: {
  empty: string
  error?: boolean
  loading: boolean
  missing: boolean
  onRetry?: () => void
  text: string | null
}) {
  const { t } = useTranslation('tasks')
  if (loading) return <PanelSkeleton label={t('workbench.artifact.loading')} />
  if (error) {
    return (
      <PanelState>
        <span>{t('workbench.artifact.loadFailed')}</span>
        {onRetry ? (
          <Button onClick={onRetry} size="sm" type="button" variant="outline">
            {t('workbench.artifact.retry')}
          </Button>
        ) : null}
      </PanelState>
    )
  }
  if (missing) return <PanelState>{t('workbench.artifact.unavailable')}</PanelState>
  if (text === null) return <PanelState>{empty}</PanelState>
  return (
    <pre className="min-h-full overflow-auto whitespace-pre-wrap p-4 font-mono text-xs leading-6">
      {text}
    </pre>
  )
}

function PanelState({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex min-h-48 flex-col items-center justify-center gap-3 px-6 text-center text-muted-foreground text-sm">
      {children}
    </div>
  )
}

function PanelSkeleton({ label }: { label: string }) {
  return (
    <div aria-label={label} className="space-y-3 p-4" role="status">
      <span className="sr-only">{label}</span>
      <div className="h-3 w-2/3 animate-pulse rounded bg-muted" />
      <div className="h-3 w-full animate-pulse rounded bg-muted" />
      <div className="h-3 w-5/6 animate-pulse rounded bg-muted" />
      <div className="h-3 w-1/2 animate-pulse rounded bg-muted" />
    </div>
  )
}
