import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { formatTime } from '@/shared/formatters'
import {
  DEFAULT_MEMORY_TENANT_ID,
  getMemoryRecallTrace,
  listMemoryRecallTraces,
} from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'
import { Button } from '@/shared/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/shared/ui/card'
import { EmptyState } from '@/shared/ui/empty-state'
import { Section } from '@/shared/ui/section'

const traceQueryKeys = {
  all: ['memory-traces'] as const,
  detail: (traceId: string | null) => ['memory-traces', 'detail', traceId] as const,
}

export function MemoryRecallTracePanel() {
  const { t } = useTranslation('memory')
  const commandClient = useCommandClient()
  const [selectedTraceId, setSelectedTraceId] = useState<string | null>(null)

  const tracesQuery = useQuery({
    queryKey: traceQueryKeys.all,
    queryFn: () =>
      listMemoryRecallTraces({ limit: 30, tenantId: DEFAULT_MEMORY_TENANT_ID }, commandClient),
  })
  const traceDetailQuery = useQuery({
    enabled: selectedTraceId !== null,
    queryKey: traceQueryKeys.detail(selectedTraceId),
    queryFn: () =>
      getMemoryRecallTrace(
        { tenantId: DEFAULT_MEMORY_TENANT_ID, traceId: selectedTraceId ?? '' },
        commandClient,
      ),
  })

  if (tracesQuery.isLoading) {
    return <div className="text-muted-foreground text-sm">{t('loading')}</div>
  }
  if (tracesQuery.isError) {
    return <div className="text-destructive text-sm">{t('errorLoading')}</div>
  }

  const traces = tracesQuery.data?.traces ?? []

  if (traces.length === 0) {
    return <EmptyState>{t('noTracesYet')}</EmptyState>
  }

  return (
    <Section>
      {traces.map((trace) => (
        <Card key={trace.trace_id}>
          <CardHeader className="flex flex-row items-center justify-between gap-3">
            <CardTitle className="text-xs font-mono">{trace.trace_id.slice(0, 12)}…</CardTitle>
            <Button
              size="sm"
              variant={selectedTraceId === trace.trace_id ? 'secondary' : 'outline'}
              onClick={() => setSelectedTraceId(trace.trace_id)}
            >
              {t('inspectTrace')}
            </Button>
          </CardHeader>
          <CardContent>
            <div className="grid grid-cols-2 gap-2 text-xs text-muted-foreground">
              <div>
                {t('injected')}: {trace.injected_count}
              </div>
              <div>
                {t('dropped')}: {trace.dropped_count}
              </div>
              <div>
                {t('redacted')}: {trace.redacted_count}
              </div>
              <div>
                {t('at')}: {formatTime(trace.at)}
              </div>
            </div>
            {selectedTraceId === trace.trace_id && (
              <TraceDetail
                error={traceDetailQuery.isError}
                loading={traceDetailQuery.isLoading}
                trace={traceDetailQuery.data?.trace}
              />
            )}
          </CardContent>
        </Card>
      ))}
    </Section>
  )
}

function TraceDetail({
  error,
  loading,
  trace,
}: {
  error: boolean
  loading: boolean
  trace: NonNullable<Awaited<ReturnType<typeof getMemoryRecallTrace>>['trace']> | undefined
}) {
  const { t } = useTranslation('memory')

  if (loading) {
    return <div className="mt-3 text-muted-foreground text-xs">{t('detailLoading')}</div>
  }
  if (error) {
    return <div className="mt-3 text-destructive text-xs">{t('errorLoading')}</div>
  }
  if (!trace) {
    return null
  }

  return (
    <div className="mt-4 space-y-3 border-border border-t pt-3 text-xs">
      <div className="grid grid-cols-2 gap-2 text-muted-foreground">
        <div>
          {t('providers')}: {trace.provider_results.length}
        </div>
        <div>
          {t('candidates')}: {trace.candidates.length}
        </div>
        <div>
          {t('injectedChars')}: {trace.injected_chars}
        </div>
        <div>
          {t('deadlineUsedMs')}: {trace.deadline_used_ms}
        </div>
      </div>

      <div className="space-y-2">
        {trace.provider_results.map((provider) => (
          <div
            className="grid grid-cols-4 gap-2 rounded-md border border-border p-2"
            key={provider.provider_id}
          >
            <div className="font-mono">{provider.provider_id}</div>
            <div>
              {t('returned')}: {provider.returned_count}
            </div>
            <div>
              {t('latencyMs')}: {provider.latency_ms}
            </div>
            <div>
              {provider.timed_out ? t('timedOut') : (provider.error_kind ?? provider.trust_level)}
            </div>
          </div>
        ))}
      </div>

      <div className="space-y-2">
        {trace.candidates.map((candidate) => (
          <div className="rounded-md border border-border p-2" key={candidate.memory_id}>
            <div className="flex items-center justify-between gap-3">
              <span className="truncate font-mono">{candidate.memory_id}</span>
              <span>{candidate.score.final_score.toFixed(2)}</span>
            </div>
            <div className="mt-1 text-muted-foreground">
              {candidate.provider_id} / {formatPolicyDecision(candidate.policy_decision)}
            </div>
            <dl className="mt-2 grid grid-cols-2 gap-x-3 gap-y-1 text-muted-foreground text-xs sm:grid-cols-4">
              <ScoreTerm label="lexical" value={candidate.score.lexical_score} />
              <ScoreTerm label="vector" value={candidate.score.vector_score} />
              <ScoreTerm label="recency" value={candidate.score.recency_score} />
              <ScoreTerm label="confidence" value={candidate.score.confidence_score} />
              <ScoreTerm label="access" value={candidate.score.access_score} />
              <ScoreTerm label="source" value={candidate.score.source_trust_score} />
              <ScoreTerm label="boost" value={candidate.score.explicit_selection_boost} />
              <ScoreTerm label="final" value={candidate.score.final_score} />
            </dl>
          </div>
        ))}
      </div>
    </div>
  )
}

function ScoreTerm({ label, value }: { label: string; value?: number | null }) {
  return (
    <div>
      <dt className="inline">{label}: </dt>
      <dd className="inline font-mono">{typeof value === 'number' ? value.toFixed(2) : '-'}</dd>
    </div>
  )
}

function formatPolicyDecision(decision: unknown): string {
  if (decision === 'allow') {
    return 'allow'
  }
  return JSON.stringify(decision)
}
