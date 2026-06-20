import { BarChart3 } from 'lucide-react'
import { useTranslation } from 'react-i18next'

export type UsageSummaryModel = {
  cacheReadTokens?: number
  cacheWriteTokens?: number
  costMicros?: number
  inputTokens: number
  outputTokens: number
  providerLabel?: string
  toolCalls: number
}

type UsageSummaryProps = {
  unavailable?: boolean
  usage?: UsageSummaryModel
}

const secretPatterns = [
  /\bAuthorization:?\s*Bearer\s+\S+/i,
  /\b(?:api[_-]?key|token|secret|password)\b\s*(?:=|\s+)\s*\S+/i,
  /\bsk-[A-Za-z0-9_-]{12,}/i,
  /\bgh[pousr]_[A-Za-z0-9_]{20,}/i,
]

function hasObviousSecret(value: string): boolean {
  return secretPatterns.some((pattern) => pattern.test(value))
}

function formatNumber(value: number, locale: string): string {
  return new Intl.NumberFormat(locale).format(value)
}

function formatCost(costMicros: number | undefined, notEstimatedLabel: string): string {
  if (costMicros === undefined) {
    return notEstimatedLabel
  }

  return `$${(costMicros / 1_000_000).toFixed(6)}`
}

function providerLabel(
  value: string | undefined,
  unavailableLabel: string,
  redactedLabel: string,
): string {
  if (!value) {
    return unavailableLabel
  }

  if (hasObviousSecret(value)) {
    return redactedLabel
  }

  return value
}

export function UsageSummary({ unavailable = false, usage }: UsageSummaryProps) {
  const { i18n, t } = useTranslation('activity')

  if (unavailable || !usage) {
    return (
      <section
        aria-label={t('usage.title')}
        className="space-y-2 rounded-md border border-border bg-surface px-4 py-3 text-sm"
      >
        <div className="flex items-center gap-2 font-medium">
          <BarChart3 className="size-4 text-muted-foreground" />
          {t('usage.unavailable')}
        </div>
        <p className="text-muted-foreground">{t('usage.permissionsUnchanged')}</p>
      </section>
    )
  }

  return (
    <section
      aria-label={t('usage.title')}
      className="space-y-4 rounded-md border border-border bg-surface px-4 py-3"
    >
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2 font-medium text-sm">
          <BarChart3 className="size-4 text-muted-foreground" />
          {t('usage.title')}
        </div>
        <span className="rounded-md border border-border bg-background px-2 py-1 text-muted-foreground text-xs">
          {providerLabel(
            usage.providerLabel,
            t('usage.providerUnavailable'),
            t('usage.providerRedacted'),
          )}
        </span>
      </div>

      <dl className="grid gap-3 sm:grid-cols-4">
        <UsageMetric
          label={t('usage.inputTokens')}
          value={formatNumber(usage.inputTokens, i18n.language)}
        />
        <UsageMetric
          label={t('usage.outputTokens')}
          value={formatNumber(usage.outputTokens, i18n.language)}
        />
        <UsageMetric
          label={t('usage.toolCalls')}
          value={formatNumber(usage.toolCalls, i18n.language)}
        />
        <UsageMetric
          label={t('usage.localCost')}
          value={formatCost(usage.costMicros, t('usage.notEstimated'))}
        />
      </dl>

      {usage.cacheReadTokens !== undefined || usage.cacheWriteTokens !== undefined ? (
        <div className="flex flex-wrap gap-3 text-muted-foreground text-xs">
          {usage.cacheReadTokens !== undefined ? (
            <span>
              {t('usage.cacheRead', { count: formatNumber(usage.cacheReadTokens, i18n.language) })}
            </span>
          ) : null}
          {usage.cacheWriteTokens !== undefined ? (
            <span>
              {t('usage.cacheWrite', {
                count: formatNumber(usage.cacheWriteTokens, i18n.language),
              })}
            </span>
          ) : null}
        </div>
      ) : null}
    </section>
  )
}

function UsageMetric({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md border border-border bg-background px-3 py-2">
      <dt className="text-muted-foreground text-xs">{label}</dt>
      <dd className="mt-1 font-mono text-sm">{value}</dd>
    </div>
  )
}
