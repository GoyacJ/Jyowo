import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'

import { listMemoryRecallTraces } from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'
import { Card, CardContent, CardHeader, CardTitle } from '@/shared/ui/card'

const traceQueryKeys = {
  all: ['memory-traces'] as const,
}

export function MemoryRecallTracePanel() {
  const { t } = useTranslation('memory')
  const commandClient = useCommandClient()

  const tracesQuery = useQuery({
    queryKey: traceQueryKeys.all,
    queryFn: () => listMemoryRecallTraces({ limit: 30 }, commandClient),
  })

  if (tracesQuery.isLoading) {
    return <div className="p-4 text-muted-foreground">{t('loading')}</div>
  }
  if (tracesQuery.isError) {
    return <div className="p-4 text-destructive">{t('errorLoading')}</div>
  }

  const traces = tracesQuery.data?.traces ?? []

  if (traces.length === 0) {
    return (
      <div className="p-4 text-muted-foreground">{t('noTracesYet')}</div>
    )
  }

  return (
    <div className="space-y-3 p-4">
      {traces.map((trace) => (
        <Card key={trace.trace_id}>
          <CardHeader>
            <CardTitle className="text-xs font-mono">
              {trace.trace_id.slice(0, 12)}...
            </CardTitle>
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
                {t('at')}: {new Date(trace.at).toLocaleTimeString()}
              </div>
            </div>
          </CardContent>
        </Card>
      ))}
    </div>
  )
}
