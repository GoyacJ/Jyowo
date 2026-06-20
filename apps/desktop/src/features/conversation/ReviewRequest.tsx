import { useTranslation } from 'react-i18next'

import { Button } from '@/shared/ui/button'

export interface ReviewRequestProps {
  continueActionLabel: string
  disabled?: boolean
  onContinue?: () => void
  title: string
}

export function ReviewRequest({
  continueActionLabel,
  disabled = false,
  onContinue,
  title,
}: ReviewRequestProps) {
  const { t } = useTranslation('conversation')

  return (
    <section
      aria-label={t('reviewRequest')}
      className="mt-4 flex items-center justify-between gap-3 rounded-md border border-border bg-surface px-4 py-3"
    >
      <span className="font-medium text-sm">{title}</span>
      <Button disabled={disabled} size="sm" type="button" variant="outline" onClick={onContinue}>
        {continueActionLabel}
      </Button>
    </section>
  )
}
