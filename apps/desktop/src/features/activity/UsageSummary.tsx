import { BarChart3 } from 'lucide-react'

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

function formatNumber(value: number): string {
  return new Intl.NumberFormat('en-US').format(value)
}

function formatCost(costMicros: number | undefined): string {
  if (costMicros === undefined) {
    return 'Not estimated'
  }

  return `$${(costMicros / 1_000_000).toFixed(6)}`
}

function providerLabel(value: string | undefined): string {
  if (!value) {
    return 'Provider unavailable'
  }

  if (hasObviousSecret(value)) {
    return 'Provider redacted'
  }

  return value
}

export function UsageSummary({ unavailable = false, usage }: UsageSummaryProps) {
  if (unavailable || !usage) {
    return (
      <section
        aria-label="Usage summary"
        className="space-y-2 rounded-md border border-border bg-surface px-4 py-3 text-sm"
      >
        <div className="flex items-center gap-2 font-medium">
          <BarChart3 className="size-4 text-muted-foreground" />
          Usage analytics unavailable.
        </div>
        <p className="text-muted-foreground">Execution permissions are unchanged.</p>
      </section>
    )
  }

  return (
    <section
      aria-label="Usage summary"
      className="space-y-4 rounded-md border border-border bg-surface px-4 py-3"
    >
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2 font-medium text-sm">
          <BarChart3 className="size-4 text-muted-foreground" />
          Usage summary
        </div>
        <span className="rounded-md border border-border bg-background px-2 py-1 text-muted-foreground text-xs">
          {providerLabel(usage.providerLabel)}
        </span>
      </div>

      <dl className="grid gap-3 sm:grid-cols-4">
        <UsageMetric label="Input tokens" value={formatNumber(usage.inputTokens)} />
        <UsageMetric label="Output tokens" value={formatNumber(usage.outputTokens)} />
        <UsageMetric label="Tool calls" value={formatNumber(usage.toolCalls)} />
        <UsageMetric label="Local cost" value={formatCost(usage.costMicros)} />
      </dl>

      {usage.cacheReadTokens !== undefined || usage.cacheWriteTokens !== undefined ? (
        <div className="flex flex-wrap gap-3 text-muted-foreground text-xs">
          {usage.cacheReadTokens !== undefined ? (
            <span>Cache read {formatNumber(usage.cacheReadTokens)}</span>
          ) : null}
          {usage.cacheWriteTokens !== undefined ? (
            <span>Cache write {formatNumber(usage.cacheWriteTokens)}</span>
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
