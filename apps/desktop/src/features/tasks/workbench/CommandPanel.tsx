import { useTranslation } from 'react-i18next'

import { ArtifactText } from './DiffPanel'

export function CommandPanel({
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
      empty={t('workbench.empty.commands')}
      loading={loading}
      missing={missing}
      text={text}
    />
  )
}
