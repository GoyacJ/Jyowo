import { ShieldAlert } from 'lucide-react'

import { Button } from '@/shared/ui/button'

type PermissionRisk = 'critical' | 'high' | 'low' | 'medium'
type PermissionState = 'approved' | 'denied' | 'pending'

export type PermissionRequestDetails = {
  command?: PermissionCommandDetails
  decisionScope?: string
  diffSummary?: string
  exposure?: string
  id: string
  label: string
  operation?: string
  reason?: string
  risk: PermissionRisk
  state: PermissionState
  target?: string
  workspaceBoundary?: string
}

type PermissionCommandDetails = {
  args?: string[]
  cwd?: string
  executable: string
  risk?: PermissionRisk
}

type PermissionDialogProps = {
  onApprove?: (permissionId: string) => void
  onDeny?: (permissionId: string) => void
  permission: PermissionRequestDetails
  resolving?: boolean
}

const riskLabels = {
  critical: 'Critical risk',
  high: 'High risk',
  low: 'Low risk',
  medium: 'Medium risk',
} satisfies Record<PermissionRisk, string>

const stateLabels = {
  approved: 'Approved',
  denied: 'Denied',
  pending: 'Pending approval',
} satisfies Record<PermissionState, string>

export function PermissionDialog({
  onApprove,
  onDeny,
  permission,
  resolving = false,
}: PermissionDialogProps) {
  const hasActions = permission.state === 'pending' && (onApprove || onDeny)

  return (
    <fieldset className="rounded-md border border-border p-4">
      <legend className="font-medium" id={`${permission.id}-title`}>
        {permission.label}
      </legend>
      <div className="flex items-start gap-3">
        <ShieldAlert aria-hidden="true" className="mt-0.5 size-4 text-muted-foreground" />
        <div className="min-w-0 flex-1">
          <div className="mt-1 flex items-center gap-2 text-muted-foreground text-xs">
            <span>{riskLabels[permission.risk]}</span>
            <span>{stateLabels[permission.state]}</span>
          </div>
          <dl className="mt-4 grid gap-3 text-sm">
            <OptionalDetail label="Operation" value={permission.operation} />
            <OptionalDetail label="Target" value={permission.target} />
            <OptionalDetail label="Reason" value={permission.reason} />
            <OptionalDetail label="Workspace boundary" value={permission.workspaceBoundary} />
            <OptionalDetail label="Exposure" value={permission.exposure} />
            <OptionalDetail label="Decision scope" value={permission.decisionScope} />
            <OptionalDetail label="Diff" value={permission.diffSummary} />
          </dl>
          {permission.command ? <PermissionCommand command={permission.command} /> : null}
        </div>
      </div>
      {hasActions ? (
        <div className="mt-4 flex justify-end gap-2">
          {onDeny ? (
            <Button
              disabled={resolving}
              onClick={() => onDeny(permission.id)}
              size="sm"
              type="button"
              variant="outline"
            >
              Deny permission
            </Button>
          ) : null}
          {onApprove ? (
            <Button
              disabled={resolving}
              onClick={() => onApprove(permission.id)}
              size="sm"
              type="button"
              variant="destructive"
            >
              Approve permission
            </Button>
          ) : null}
        </div>
      ) : null}
    </fieldset>
  )
}

function OptionalDetail({ label, value }: { label: string; value?: string }) {
  if (!value) {
    return null
  }

  return (
    <div className="grid gap-1">
      <dt className="text-muted-foreground text-xs">{label}</dt>
      <dd className="break-words">{value}</dd>
    </div>
  )
}

function PermissionCommand({ command }: { command: PermissionCommandDetails }) {
  return (
    <div className="mt-4 rounded-md bg-muted p-3 text-sm">
      <div className="font-medium">Command</div>
      <dl className="mt-3 grid gap-3">
        <OptionalDetail label="Executable" value={command.executable} />
        {command.args?.length ? (
          <div className="grid gap-1">
            <dt className="text-muted-foreground text-xs">Args</dt>
            <dd className="flex flex-wrap gap-1 font-mono text-xs">
              {command.args.map((arg, index) => (
                <span className="rounded-md border border-border px-2 py-1" key={`${index}:${arg}`}>
                  {formatArg(arg)}
                </span>
              ))}
            </dd>
          </div>
        ) : null}
        <OptionalDetail label="Cwd" value={command.cwd} />
      </dl>
    </div>
  )
}

function formatArg(arg: string) {
  return /\s/.test(arg) ? JSON.stringify(arg) : arg
}
