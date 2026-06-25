import { useTranslation } from 'react-i18next'
import type { ThinkingSegment } from '@/shared/tauri/commands'

export function ThinkingPanel({ segment }: { segment: ThinkingSegment }) {
  const { t } = useTranslation('conversation')

  return (
    <details className="rounded-md border border-border bg-muted/40 px-3 py-2 text-sm">
      <summary className="cursor-pointer text-muted-foreground">
        {t('timeline.thinkingSummary')}
      </summary>
      <p className="mt-2 whitespace-pre-wrap text-muted-foreground">{segment.summary.text}</p>
    </details>
  )
}
