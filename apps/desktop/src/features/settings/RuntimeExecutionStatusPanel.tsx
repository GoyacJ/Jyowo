import { useQuery } from '@tanstack/react-query'
import { Activity, AlertTriangle, CheckCircle2, XCircle } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import {
  getRuntimeExecutionStatus,
  type RuntimeExecutionStatus,
  type ToolRuntimeStatus,
} from '@/shared/tauri/commands'
import { getCommandErrorMessage } from '@/shared/tauri/errors'
import { useCommandClient } from '@/shared/tauri/react'
import { Badge } from '@/shared/ui/badge'

export function RuntimeExecutionStatusPanel() {
  const { t } = useTranslation('settings')
  const commandClient = useCommandClient()
  const statusQuery = useQuery({
    queryKey: ['settings', 'runtime-execution-status'],
    queryFn: () => getRuntimeExecutionStatus(commandClient),
  })

  return (
    <section className="rounded-md border border-border bg-surface">
      <div className="flex items-start gap-3 border-border border-b p-5">
        <div className="rounded-md border border-border bg-background p-2 text-muted-foreground">
          <Activity className="size-4" />
        </div>
        <div>
          <h2 className="font-semibold text-base">{t('runtimeStatus.title')}</h2>
          <p className="mt-1 text-muted-foreground text-sm">{t('runtimeStatus.description')}</p>
        </div>
      </div>

      {statusQuery.isLoading ? (
        <p className="p-5 text-muted-foreground text-sm">{t('runtimeStatus.loading')}</p>
      ) : statusQuery.isError ? (
        <div className="flex items-start gap-2 p-5 text-destructive text-sm">
          <AlertTriangle className="mt-0.5 size-4 shrink-0" />
          <span>{getCommandErrorMessage(statusQuery.error)}</span>
        </div>
      ) : statusQuery.data ? (
        <RuntimeExecutionStatusBody status={statusQuery.data} />
      ) : null}
    </section>
  )
}

function RuntimeExecutionStatusBody({ status }: { status: RuntimeExecutionStatus }) {
  const { t } = useTranslation('settings')

  return (
    <div className="space-y-5 p-5">
      <dl className="grid gap-4 md:grid-cols-3">
        <div className="space-y-2">
          <dt className="font-medium text-muted-foreground text-xs uppercase">
            {t('runtimeStatus.processBackend')}
          </dt>
          <dd className="font-mono text-sm">{status.processSandbox.backendId}</dd>
        </div>
        <div className="space-y-2">
          <dt className="font-medium text-muted-foreground text-xs uppercase">
            {t('runtimeStatus.candidates')}
          </dt>
          <dd className="flex flex-wrap gap-1.5">
            {status.processSandbox.candidateIds.map((candidateId) => (
              <Badge className="font-mono" key={candidateId} variant="outline">
                {candidateId}
              </Badge>
            ))}
          </dd>
        </div>
        <div className="space-y-2">
          <dt className="font-medium text-muted-foreground text-xs uppercase">
            {t('runtimeStatus.httpBroker')}
          </dt>
          <dd>
            <AvailabilityBadge available={status.httpBroker.available} />
          </dd>
        </div>
      </dl>

      <PolicyList
        label={t('runtimeStatus.networkPolicies')}
        values={status.processSandbox.availableNetworkPolicies}
      />
      <PolicyList
        label={t('runtimeStatus.workspacePolicies')}
        values={status.processSandbox.availableWorkspacePolicies}
      />

      <ReasonList reasons={status.processSandbox.unavailableReasons} />
      <ReasonList reasons={status.httpBroker.deniedReasons} />

      <div className="overflow-x-auto">
        <table className="w-full min-w-[560px] border-collapse text-left text-sm">
          <thead className="bg-background text-muted-foreground">
            <tr className="border-border border-b">
              <th className="px-3 py-2 font-medium">{t('runtimeStatus.tool')}</th>
              <th className="px-3 py-2 font-medium">{t('runtimeStatus.status')}</th>
              <th className="px-3 py-2 font-medium">{t('runtimeStatus.reason')}</th>
            </tr>
          </thead>
          <tbody>
            {status.tools.map((tool) => (
              <ToolStatusRow key={tool.toolName} tool={tool} />
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}

function PolicyList({ label, values }: { label: string; values: string[] }) {
  return (
    <div className="space-y-2">
      <div className="font-medium text-muted-foreground text-xs uppercase">{label}</div>
      <div className="flex flex-wrap gap-1.5">
        {values.map((value) => (
          <Badge className="font-mono" key={value} variant="secondary">
            {value}
          </Badge>
        ))}
      </div>
    </div>
  )
}

function ReasonList({ reasons }: { reasons: string[] }) {
  if (reasons.length === 0) {
    return null
  }

  return (
    <ul className="space-y-1 text-muted-foreground text-sm">
      {reasons.map((reason) => (
        <li className="flex items-start gap-2" key={reason}>
          <AlertTriangle className="mt-0.5 size-4 shrink-0 text-destructive" />
          <span>{reason}</span>
        </li>
      ))}
    </ul>
  )
}

function ToolStatusRow({ tool }: { tool: ToolRuntimeStatus }) {
  return (
    <tr className="border-border border-b last:border-b-0">
      <td className="px-3 py-2 align-top font-medium">{tool.toolName}</td>
      <td className="px-3 py-2 align-top">
        <AvailabilityBadge available={tool.available} />
      </td>
      <td className="px-3 py-2 align-top text-muted-foreground">{tool.unavailableReason ?? ''}</td>
    </tr>
  )
}

function AvailabilityBadge({ available }: { available: boolean }) {
  const { t } = useTranslation('settings')
  const Icon = available ? CheckCircle2 : XCircle

  return (
    <Badge variant={available ? 'success' : 'destructive'}>
      <Icon className="size-3" />
      {available ? t('runtimeStatus.available') : t('runtimeStatus.unavailable')}
    </Badge>
  )
}
