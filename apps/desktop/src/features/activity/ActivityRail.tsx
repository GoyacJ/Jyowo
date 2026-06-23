import { Settings } from 'lucide-react'
import { useTranslation } from 'react-i18next'

type ActivityRailProps = {
  activeRunId?: string
  errorMessage?: string
  onOpenSettings?: () => void
}

export function ActivityRail({ activeRunId, errorMessage, onOpenSettings }: ActivityRailProps) {
  const { t } = useTranslation(['activity', 'common', 'shell'])
  const running = Boolean(activeRunId)

  return (
    <section
      aria-label={t('activity:statusBar.title')}
      className="flex h-full min-w-0 items-center justify-between gap-4 border-border border-t bg-muted/45 px-3 text-muted-foreground text-xs"
    >
      <div className="flex min-w-0 items-center gap-4">
        <button
          aria-label={t('shell:nav.settings')}
          className="grid size-7 place-items-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
          onClick={onOpenSettings}
          title={t('shell:nav.settings')}
          type="button"
        >
          <Settings className="size-3.5" />
        </button>
        <span className="flex items-center gap-2 text-foreground">
          <span
            aria-hidden="true"
            className={`size-1.5 rounded-full ${running ? 'bg-warning' : 'bg-success'}`}
          />
          {running ? t('common:status.running') : t('common:status.ready')}
        </span>
        <span>{t('common:local')}</span>
      </div>

      <div className="min-w-0 truncate font-mono">
        {running ? (
          <>
            <span className="mr-2 font-sans text-muted-foreground">
              {t('activity:statusBar.activeRun')}
            </span>
            <span
              className={errorMessage ? 'font-sans text-destructive' : 'font-sans text-foreground'}
            >
              {errorMessage ?? t('common:status.inProgress')}
            </span>
          </>
        ) : (
          <span className="font-sans">{t('activity:statusBar.idle')}</span>
        )}
      </div>
    </section>
  )
}
