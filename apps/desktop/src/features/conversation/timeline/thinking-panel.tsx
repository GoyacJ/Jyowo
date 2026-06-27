import { useTranslation } from 'react-i18next'
import type { ThinkingSegment } from '@/shared/tauri/commands'

export function ThinkingPanel({ segment }: { segment: ThinkingSegment }) {
  const { t } = useTranslation('conversation')
  const steps = [...(segment.steps ?? [])].sort((left, right) => left.order - right.order)

  return (
    <details className="rounded-md border border-border bg-muted/40 px-3 py-2 text-sm">
      <summary className="cursor-pointer text-muted-foreground">
        {t('timeline.reasoningProcess')}
      </summary>
      <p className="mt-2 whitespace-pre-wrap text-muted-foreground">{segment.summary.text}</p>
      {steps.length > 0 ? (
        <ol className="mt-3 grid gap-2 text-muted-foreground">
          {steps.map((step) => (
            <li key={step.id} className="grid gap-1 border-border border-l pl-3">
              <div className="font-medium text-foreground text-sm">
                {step.status === 'withheld' ? t('timeline.thinkingWithheld') : step.title}
              </div>
              {step.status !== 'withheld' && step.body ? (
                <p className="whitespace-pre-wrap text-sm leading-6">{step.body}</p>
              ) : null}
            </li>
          ))}
        </ol>
      ) : null}
    </details>
  )
}
