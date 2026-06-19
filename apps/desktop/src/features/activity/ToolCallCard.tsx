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

const statusLabels = {
  blocked: 'Blocked',
  failed: 'Failed',
  queued: 'Queued',
  redacted: 'Redacted',
  running: 'Running',
  success: 'Success',
} satisfies Record<ToolStatus, string>

const statusVariants = {
  blocked: 'outline',
  failed: 'destructive',
  queued: 'secondary',
  redacted: 'outline',
  running: 'outline',
  success: 'success',
} satisfies Record<ToolStatus, 'destructive' | 'outline' | 'secondary' | 'success'>

const permissionStateLabels = {
  approved: 'Permission approved',
  denied: 'Permission denied',
  not_required: 'Permission not required',
  pending: 'Permission pending',
} satisfies Record<ToolPermissionState, string>

export function ToolCallCard({ toolCall }: { toolCall: ToolCallDetails }) {
  return (
    <section aria-labelledby="tool-call-title" className="rounded-md border border-border p-4">
      <div className="flex items-center justify-between gap-3">
        <h3 className="font-medium" id="tool-call-title">
          {toolCall.toolName}
        </h3>
        <div className="flex items-center gap-2">
          <Badge variant={statusVariants[toolCall.status]}>{statusLabels[toolCall.status]}</Badge>
          {toolCall.durationMs ? (
            <span className="font-mono text-muted-foreground text-xs">
              {formatDuration(toolCall.durationMs)}
            </span>
          ) : null}
        </div>
      </div>
      <dl className="mt-4 grid gap-3 text-sm">
        {toolCall.startedAt ? <DetailRow label="Started" value={toolCall.startedAt} /> : null}
        {toolCall.endedAt ? <DetailRow label="Ended" value={toolCall.endedAt} /> : null}
        {toolCall.permissionState ? (
          <DetailRow label="Permission" value={permissionStateLabels[toolCall.permissionState]} />
        ) : null}
        <DetailRow label="Arguments" value={toolCall.argumentsSummary} />
        {toolCall.outputSummary ? (
          <DetailRow label="Output" value={toolCall.outputSummary} />
        ) : null}
        {toolCall.errorDetails ? <DetailRow label="Error" value={toolCall.errorDetails} /> : null}
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
