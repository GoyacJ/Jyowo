import { ShieldAlert } from 'lucide-react'
import { useTranslation } from 'react-i18next'

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

const riskLabelKeys = {
  critical: 'command.risk.critical',
  high: 'command.risk.high',
  low: 'command.risk.low',
  medium: 'command.risk.medium',
} satisfies Record<PermissionRisk, string>

const stateLabelKeys = {
  approved: 'permission.state.approved',
  denied: 'permission.state.denied',
  pending: 'permission.state.pending',
} satisfies Record<PermissionState, string>

export function PermissionDialog({
  onApprove,
  onDeny,
  permission,
  resolving = false,
}: PermissionDialogProps) {
  const { t } = useTranslation('activity')
  const hasActions = permission.state === 'pending' && (onApprove || onDeny)
  const title = permission.label === 'permission' ? t('eventLabels.permission') : permission.label

  return (
    <fieldset className="rounded-md border border-border p-4">
      <legend className="font-medium" id={`${permission.id}-title`}>
        {title}
      </legend>
      <div className="flex items-start gap-3">
        <ShieldAlert aria-hidden="true" className="mt-0.5 size-4 text-muted-foreground" />
        <div className="min-w-0 flex-1">
          <div className="mt-1 flex items-center gap-2 text-muted-foreground text-xs">
            <span>{t(riskLabelKeys[permission.risk])}</span>
            <span>{t(stateLabelKeys[permission.state])}</span>
          </div>
          <dl className="mt-4 grid gap-3 text-sm">
            <OptionalDetail label={t('permission.operation')} value={permission.operation} />
            <OptionalDetail label={t('permission.target')} value={permission.target} />
            <OptionalDetail label={t('permission.reason')} value={permission.reason} />
            <OptionalDetail
              label={t('permission.workspaceBoundary')}
              value={permission.workspaceBoundary}
            />
            <OptionalDetail label={t('permission.exposure')} value={permission.exposure} />
            <OptionalDetail
              label={t('permission.decisionScope')}
              value={permission.decisionScope}
            />
            <OptionalDetail label={t('permission.diff')} value={permission.diffSummary} />
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
              {t('permission.deny')}
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
              {t('permission.approve')}
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
  const { t } = useTranslation('activity')

  return (
    <div className="mt-4 rounded-md bg-muted p-3 text-sm">
      <div className="font-medium">{t('command.title')}</div>
      <dl className="mt-3 grid gap-3">
        <OptionalDetail label={t('command.executable')} value={command.executable} />
        {command.args?.length ? (
          <div className="grid gap-1">
            <dt className="text-muted-foreground text-xs">{t('command.args')}</dt>
            <dd className="flex flex-wrap gap-1 font-mono text-xs">
              {command.args.map((arg, index) => (
                <span className="rounded-md border border-border px-2 py-1" key={`${index}:${arg}`}>
                  {formatArg(arg)}
                </span>
              ))}
            </dd>
          </div>
        ) : null}
        <OptionalDetail label={t('command.cwd')} value={command.cwd} />
      </dl>
    </div>
  )
}

function formatArg(arg: string) {
  return /\s/.test(arg) ? JSON.stringify(arg) : arg
}
