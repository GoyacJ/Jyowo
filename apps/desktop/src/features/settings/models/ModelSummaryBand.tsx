import { Activity, Gauge, Layers3, ShieldCheck } from 'lucide-react'
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import { Badge } from '@/shared/ui/badge'

import type { ModelSettingsSummaryView } from './model-settings-view-model'

type ModelSummaryBandProps = {
  summary: ModelSettingsSummaryView
}

export function ModelSummaryBand({ summary }: ModelSummaryBandProps) {
  const { t } = useTranslation('settings')

  return (
    <section
      aria-label={t('models.summary.label')}
      className="grid gap-3 rounded-md border border-border bg-surface p-4 sm:grid-cols-2 xl:grid-cols-4"
    >
      <SummaryMetric
        icon={<ShieldCheck aria-hidden="true" className="size-4" data-icon />}
        label={t('models.summary.defaultModel')}
        value={
          summary.defaultModel.status === 'ready'
            ? summary.defaultModel.data.displayName
            : t('models.unavailable')
        }
        detail={
          summary.defaultModel.status === 'ready'
            ? summary.defaultModel.data.providerDisplayName
            : t('models.summary.noDefault')
        }
      />
      <SummaryMetric
        icon={<Activity aria-hidden="true" className="size-4" data-icon />}
        label={t('models.summary.configuredModels')}
        value={
          summary.configuredModels.status === 'ready'
            ? t('models.summary.configuredCount', { count: summary.configuredModels.data.total })
            : summaryStatusLabel(summary.configuredModels.status, t)
        }
        detail={
          summary.configuredModels.status === 'ready' ? (
            <span className="flex flex-wrap gap-1.5">
              <Badge variant="success">
                {t('models.summary.availableCount', {
                  count: summary.configuredModels.data.available,
                })}
              </Badge>
              <Badge
                variant={summary.configuredModels.data.failing > 0 ? 'destructive' : 'outline'}
              >
                {t('models.summary.failingCount', { count: summary.configuredModels.data.failing })}
              </Badge>
            </span>
          ) : (
            summaryStatusLabel(summary.configuredModels.status, t)
          )
        }
      />
      <SummaryMetric
        icon={<Layers3 aria-hidden="true" className="size-4" data-icon />}
        label={t('models.summary.localUsage')}
        value={
          summary.localUsage.status === 'ready'
            ? formatTokens(summary.localUsage.data.today)
            : summaryStatusLabel(summary.localUsage.status, t)
        }
        detail={
          summary.localUsage.status === 'ready'
            ? `${t('models.columns.monthUsage')}: ${formatTokens(
                summary.localUsage.data.monthToDate,
              )} · ${t('models.columns.totalUsage')}: ${formatTokens(
                summary.localUsage.data.allTime,
              )}`
            : summaryStatusLabel(summary.localUsage.status, t)
        }
      />
      <SummaryMetric
        icon={<Gauge aria-hidden="true" className="size-4" data-icon />}
        label={t('models.summary.officialQuota')}
        value={
          summary.officialQuota.status === 'ready'
            ? t('models.summary.quotaSupportedCount', {
                count: summary.officialQuota.data.supported,
              })
            : summaryStatusLabel(summary.officialQuota.status, t)
        }
        detail={
          summary.officialQuota.status === 'ready'
            ? t('models.summary.quotaBreakdown', {
                unsupported: summary.officialQuota.data.unsupported,
                authRequired: summary.officialQuota.data.authRequired,
                failed: summary.officialQuota.data.failed,
              })
            : summaryStatusLabel(summary.officialQuota.status, t)
        }
      />
    </section>
  )
}

function summaryStatusLabel(
  status: Exclude<ModelSettingsSummaryView['configuredModels']['status'], 'ready'>,
  t: ReturnType<typeof useTranslation>['t'],
) {
  return status === 'loading'
    ? t('models.summary.loadingMetric')
    : t('models.summary.unavailableMetric')
}

function SummaryMetric({
  detail,
  icon,
  label,
  value,
}: {
  detail: ReactNode
  icon: ReactNode
  label: string
  value: string
}) {
  return (
    <div className="min-h-24 rounded-md border border-border bg-background p-3">
      <div className="flex items-center gap-2 text-muted-foreground text-xs">
        {icon}
        <span>{label}</span>
      </div>
      <div className="mt-2 truncate font-semibold text-base">{value}</div>
      <div className="mt-2 min-h-5 text-muted-foreground text-xs">{detail}</div>
    </div>
  )
}

function formatTokens(usage: {
  inputTokens: number
  outputTokens: number
  cacheReadTokens: number
  cacheWriteTokens: number
}) {
  const total =
    usage.inputTokens + usage.outputTokens + usage.cacheReadTokens + usage.cacheWriteTokens
  return `${new Intl.NumberFormat().format(total)} tokens`
}
