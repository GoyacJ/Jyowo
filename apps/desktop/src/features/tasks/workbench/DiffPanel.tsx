import { useTranslation } from 'react-i18next'

export function DiffPanel({
  loading,
  missing,
  text,
}: {
  loading: boolean
  missing: boolean
  text: string | null
}) {
  const { t } = useTranslation('tasks')
  return (
    <ArtifactText
      empty={t('workbench.empty.changes')}
      loading={loading}
      missing={missing}
      text={text}
    />
  )
}

export function ArtifactText({
  empty,
  loading,
  missing,
  text,
}: {
  empty: string
  loading: boolean
  missing: boolean
  text: string | null
}) {
  const { t } = useTranslation('tasks')
  if (loading) return <PanelState>{t('workbench.artifact.loading')}</PanelState>
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
    <p className="flex min-h-48 items-center justify-center px-6 text-center text-muted-foreground text-sm">
      {children}
    </p>
  )
}
