import { useTranslation } from 'react-i18next'
import type { ClarificationRequestSegment } from '@/shared/tauri/commands'

export function ClarificationRequestSegmentView({
  segment,
}: {
  segment: ClarificationRequestSegment
}) {
  const { t } = useTranslation('conversation')

  return (
    <section className="rounded-md border border-border px-3 py-2">
      <div className="font-medium text-sm">{t('timeline.clarificationNeeded')}</div>
      <p className="mt-1 text-sm">{segment.prompt}</p>
    </section>
  )
}
