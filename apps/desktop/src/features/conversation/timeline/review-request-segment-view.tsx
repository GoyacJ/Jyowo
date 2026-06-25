import { useTranslation } from 'react-i18next'
import type { ReviewRequestSegment } from '@/shared/tauri/commands'

export function ReviewRequestSegmentView({
  onContinue,
  segment,
}: {
  onContinue?: (prompt: string) => void
  segment: ReviewRequestSegment
}) {
  const { t } = useTranslation('conversation')

  return (
    <section className="rounded-md border border-border px-3 py-2">
      <div className="font-medium text-sm">{segment.title}</div>
      {segment.body ? <p className="mt-1 text-muted-foreground text-sm">{segment.body}</p> : null}
      <button
        className="mt-2 rounded-md bg-primary px-3 py-1.5 text-primary-foreground text-sm"
        onClick={() => onContinue?.(segment.body ?? segment.title)}
        type="button"
      >
        {t('continue')}
      </button>
    </section>
  )
}
