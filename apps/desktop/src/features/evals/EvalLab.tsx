import { FlaskConical, Loader2, Play, TriangleAlert } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { formatDateTime } from '@/shared/formatters'
import { cn } from '@/shared/lib/utils'
import { Button } from '@/shared/ui/button'
import { Card } from '@/shared/ui/card'
import { EmptyState } from '@/shared/ui/empty-state'
import { Section, SectionDescription, SectionHeader, SectionTitle } from '@/shared/ui/section'

type EvalRunStatus = 'failed' | 'passed' | 'running' | 'unavailable'

export type EvalCase = {
  id: string
  lastRun?: {
    completedAt?: string
    failed: number
    passed: number
    status: EvalRunStatus
  }
  title: string
}

type EvalLabProps = {
  cases: readonly EvalCase[]
  errorMessage?: string
  onRunCase?: (caseId: string) => void
  unavailable?: boolean
}

const statusStyles = {
  failed: 'text-destructive',
  passed: 'text-success',
  running: 'text-warning',
  unavailable: 'text-muted-foreground',
} satisfies Record<EvalRunStatus, string>

export function EvalLab({ cases, errorMessage, onRunCase, unavailable = false }: EvalLabProps) {
  const { t } = useTranslation('evals')

  return (
    <Section aria-label={t('lab')}>
      <SectionHeader className="flex items-start gap-3 space-y-0">
        <div className="rounded-md border border-border bg-background p-2 text-muted-foreground">
          <FlaskConical className="size-4" />
        </div>
        <div>
          <SectionTitle>{t('lab')}</SectionTitle>
          <SectionDescription>{t('description')}</SectionDescription>
        </div>
      </SectionHeader>

      {unavailable ? <EmptyState>{t('runtimeUnavailable')}</EmptyState> : null}

      {errorMessage ? (
        <div
          className="flex items-center gap-2 rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm"
          role="alert"
        >
          <TriangleAlert className="size-4" />
          {t('loadError')}
        </div>
      ) : null}

      {!unavailable && !errorMessage && cases.length === 0 ? (
        <EmptyState>{t('empty')}</EmptyState>
      ) : null}

      {cases.length > 0 ? (
        <div className="space-y-3">
          {cases.map((evalCase) => {
            const status = evalCase.lastRun?.status ?? 'unavailable'
            const isRunning = status === 'running'

            return (
              <Card
                aria-label={evalCase.title}
                className={cn(
                  'bg-background/50 px-4 py-3 hover:bg-background hover:-translate-y-[0.5px]',
                  isRunning && 'border-warning/45 bg-warning/5 ring-1 ring-warning/15',
                )}
                key={evalCase.id}
                role="article"
              >
                <div className="flex items-start justify-between gap-4">
                  <div className="min-w-0">
                    <div className="flex flex-wrap items-center gap-2.5">
                      <h3 className="font-semibold text-sm text-foreground/90">{evalCase.title}</h3>
                      <span
                        className={cn(
                          'text-xs flex items-center gap-1.5 font-medium',
                          statusStyles[status],
                        )}
                      >
                        {isRunning && (
                          <span className="relative flex size-1.5 items-center justify-center">
                            <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-warning/50 opacity-75"></span>
                            <span className="relative inline-flex size-1.5 rounded-full bg-warning"></span>
                          </span>
                        )}
                        {status}
                      </span>
                    </div>
                    {evalCase.lastRun ? (
                      <div className="mt-2 flex flex-wrap gap-3 text-muted-foreground text-xs font-medium">
                        <span>{t('passed', { count: evalCase.lastRun.passed })}</span>
                        <span>{t('failed', { count: evalCase.lastRun.failed })}</span>
                        {evalCase.lastRun.completedAt ? (
                          <span>{formatDateTime(evalCase.lastRun.completedAt)}</span>
                        ) : null}
                      </div>
                    ) : (
                      <p className="mt-2 text-muted-foreground text-xs">{t('noPreviousResult')}</p>
                    )}
                  </div>

                  <Button
                    disabled={!onRunCase || unavailable || isRunning}
                    onClick={() => onRunCase?.(evalCase.id)}
                    size="sm"
                    type="button"
                    variant="outline"
                    className="min-w-[90px]"
                  >
                    {isRunning ? (
                      <Loader2 className="size-3.5 animate-spin text-warning" />
                    ) : (
                      <Play className="size-3.5" />
                    )}
                    {t('runCase', { title: evalCase.title })}
                  </Button>
                </div>
              </Card>
            )
          })}
        </div>
      ) : null}
    </Section>
  )
}
