import { LoaderCircle } from 'lucide-react'
import { useTranslation } from 'react-i18next'

type ProgressStatus = 'blocked' | 'failed' | 'queued' | 'running' | 'success'

export interface ProgressBlockProps {
  label: string
  status: ProgressStatus
  time?: string
}

export function ProgressBlock({ label, status, time }: ProgressBlockProps) {
  const { t } = useTranslation(['common', 'conversation'])
  const statusLabel =
    status === 'success' ? t('conversation:progress.complete') : t(`common:status.${status}`)

  return (
    <section
      aria-label={t('conversation:progress.title')}
      className="mt-3 flex items-center justify-between gap-3 text-sm"
    >
      <div className="flex min-w-0 items-center gap-3">
        <LoaderCircle
          aria-hidden="true"
          className={`size-4 shrink-0 text-muted-foreground ${status === 'running' ? 'animate-spin' : ''}`}
        />
        <div className="min-w-0">
          <p className="truncate font-medium">{t('conversation:progress.working', { label })}</p>
          {time ? <p className="text-muted-foreground text-xs">{time}</p> : null}
        </div>
      </div>
      <span className="shrink-0 text-muted-foreground text-xs">{statusLabel}</span>
    </section>
  )
}
