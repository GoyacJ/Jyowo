import { useTranslation } from 'react-i18next'

import { ArtifactText } from './DiffPanel'

export function CommandPanel({
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
      empty={t('workbench.empty.commands')}
      error={error}
      loading={loading}
      missing={missing}
      onRetry={onRetry}
      text={text}
    />
  )
}
