import { useQuery } from '@tanstack/react-query'
import { RefreshCw } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { getAppInfo, getHarnessHealthcheck } from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'
import { Badge } from '@/shared/ui/badge'
import { Button } from '@/shared/ui/button'

function getErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

export function SystemStatusPage() {
  const { t } = useTranslation('system')
  const commandClient = useCommandClient()
  const appInfoQuery = useQuery({
    queryKey: ['system-status', 'app-info'],
    queryFn: () => getAppInfo(commandClient),
  })
  const healthcheckQuery = useQuery({
    queryKey: ['system-status', 'harness-healthcheck'],
    queryFn: () => getHarnessHealthcheck(commandClient),
  })

  const isLoading = appInfoQuery.isLoading || healthcheckQuery.isLoading
  const error = appInfoQuery.error ?? healthcheckQuery.error

  if (isLoading) {
    return (
      <section
        aria-labelledby="app-title"
        className="rounded-lg border border-border bg-surface p-6 shadow-sm"
      >
        <p className="text-muted-foreground text-sm">{t('loading')}</p>
      </section>
    )
  }

  if (error) {
    return (
      <section
        aria-labelledby="app-title"
        className="rounded-lg border border-destructive/30 bg-destructive/5 p-6"
      >
        <h1 id="app-title" className="font-semibold text-2xl tracking-normal">
          Jyowo
        </h1>
        <p className="mt-3 text-destructive text-sm">{getErrorMessage(error)}</p>
      </section>
    )
  }

  const appInfo = appInfoQuery.data
  const healthcheck = healthcheckQuery.data

  if (!appInfo || !healthcheck) {
    return null
  }

  return (
    <section
      aria-labelledby="app-title"
      className="rounded-lg border border-border bg-surface p-6 shadow-sm"
    >
      <div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
        <div className="flex flex-col gap-2">
          <div className="flex flex-wrap items-center gap-3">
            <h1 id="app-title" className="font-semibold text-3xl tracking-normal">
              {appInfo.name}
            </h1>
            <Badge variant="success">{healthcheck.status}</Badge>
          </div>
          <p className="text-muted-foreground text-sm">{appInfo.harness.sdkCrate}</p>
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={() => {
            void appInfoQuery.refetch()
            void healthcheckQuery.refetch()
          }}
        >
          <RefreshCw data-icon="inline-start" />
          {t('refresh')}
        </Button>
      </div>

      <dl className="mt-8 grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <div className="rounded-md border border-border bg-muted/35 p-4">
          <dt className="text-muted-foreground text-xs uppercase tracking-normal">
            {t('version')}
          </dt>
          <dd className="mt-2 overflow-wrap-anywhere font-medium">{appInfo.version}</dd>
        </div>
        <div className="rounded-md border border-border bg-muted/35 p-4">
          <dt className="text-muted-foreground text-xs uppercase tracking-normal">{t('shell')}</dt>
          <dd className="mt-2 overflow-wrap-anywhere font-medium">{appInfo.shell}</dd>
        </div>
        <div className="rounded-md border border-border bg-muted/35 p-4">
          <dt className="text-muted-foreground text-xs uppercase tracking-normal">
            {t('sdkCrate')}
          </dt>
          <dd className="mt-2 overflow-wrap-anywhere font-medium">{appInfo.harness.sdkCrate}</dd>
        </div>
        <div className="rounded-md border border-border bg-muted/35 p-4">
          <dt className="text-muted-foreground text-xs uppercase tracking-normal">{t('mode')}</dt>
          <dd className="mt-2 overflow-wrap-anywhere font-medium">{appInfo.harness.mode}</dd>
        </div>
      </dl>
    </section>
  )
}
