import { useTranslation } from 'react-i18next'

import { Badge } from '@/shared/ui/badge'

type ToolStatus = 'blocked' | 'failed' | 'queued' | 'redacted' | 'running' | 'success'
type ToolPermissionState = 'approved' | 'denied' | 'not_required' | 'pending'

export type ToolCallDetails = {
  argumentsSummary: string
  durationMs?: number
  endedAt?: string
  errorDetails?: string
  outputSummary?: string
  permissionState?: ToolPermissionState
  startedAt?: string
  status: ToolStatus
  toolName: string
}

const statusVariants = {
  blocked: 'outline',
  failed: 'destructive',
  queued: 'secondary',
  redacted: 'outline',
  running: 'outline',
  success: 'success',
} satisfies Record<ToolStatus, 'destructive' | 'outline' | 'secondary' | 'success'>

const permissionStateLabelKeys = {
  approved: 'toolCall.permissionStates.approved',
  denied: 'toolCall.permissionStates.denied',
  not_required: 'toolCall.permissionStates.notRequired',
  pending: 'toolCall.permissionStates.pending',
} satisfies Record<ToolPermissionState, string>

export function ToolCallCard({ toolCall }: { toolCall: ToolCallDetails }) {
  const { t } = useTranslation(['activity', 'common'])

  return (
    <section aria-labelledby="tool-call-title" className="rounded-md border border-border p-4">
      <div className="flex items-center justify-between gap-3">
        <h3 className="font-medium" id="tool-call-title">
          {toolCall.toolName}
        </h3>
        <div className="flex items-center gap-2">
          <Badge variant={statusVariants[toolCall.status]}>
            {t(`common:status.${toolCall.status}`)}
          </Badge>
          {toolCall.durationMs ? (
            <span className="font-mono text-muted-foreground text-xs">
              {formatDuration(toolCall.durationMs)}
            </span>
          ) : null}
        </div>
      </div>
      <dl className="mt-4 grid gap-3 text-sm">
        {toolCall.startedAt ? (
          <DetailRow label={t('activity:toolCall.started')} value={toolCall.startedAt} />
        ) : null}
        {toolCall.endedAt ? (
          <DetailRow label={t('activity:toolCall.ended')} value={toolCall.endedAt} />
        ) : null}
        {toolCall.permissionState ? (
          <DetailRow
            label={t('activity:toolCall.permission')}
            value={t(`activity:${permissionStateLabelKeys[toolCall.permissionState]}`)}
          />
        ) : null}
        <DetailRow label={t('activity:toolCall.arguments')} value={toolCall.argumentsSummary} />
        {toolCall.outputSummary ? (
          <DetailRow label={t('activity:toolCall.output')} value={toolCall.outputSummary} />
        ) : null}
        {toolCall.errorDetails ? (
          <DetailRow label={t('activity:toolCall.error')} value={toolCall.errorDetails} />
        ) : null}
      </dl>
    </section>
  )
}

function DetailRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="grid gap-1">
      <dt className="text-muted-foreground text-xs">{label}</dt>
      <dd className="break-words">{value}</dd>
    </div>
  )
}

function formatDuration(durationMs: number) {
  if (durationMs < 1000) {
    return `${durationMs}ms`
  }

  return `${(durationMs / 1000).toFixed(2)}s`
}
