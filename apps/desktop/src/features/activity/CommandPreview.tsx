import { useTranslation } from 'react-i18next'

import { Badge } from '@/shared/ui/badge'

type CommandRisk = 'critical' | 'high' | 'low' | 'medium'
type CommandApprovalState = 'approved' | 'denied' | 'pending'

type CommandEnvironmentEntry = {
  name: string
  redacted?: boolean
  value: string
}

export type CommandDetails = {
  approvalState: CommandApprovalState
  args: string[]
  cwd: string
  environment: CommandEnvironmentEntry[]
  executable: string
  risk: CommandRisk
}

const riskLabelKeys = {
  critical: 'command.risk.critical',
  high: 'command.risk.high',
  low: 'command.risk.low',
  medium: 'command.risk.medium',
} satisfies Record<CommandRisk, string>

const approvalLabelKeys = {
  approved: 'command.approval.approved',
  denied: 'command.approval.denied',
  pending: 'command.approval.pending',
} satisfies Record<CommandApprovalState, string>

export function CommandPreview({ command }: { command: CommandDetails }) {
  const { t } = useTranslation('activity')

  return (
    <section
      aria-labelledby="command-preview-title"
      className="rounded-md border border-border p-4"
    >
      <div className="flex items-center justify-between gap-3">
        <h3 className="font-medium" id="command-preview-title">
          {t('command.title')}
        </h3>
        <div className="flex items-center gap-2">
          <Badge variant={command.risk === 'critical' ? 'destructive' : 'outline'}>
            {t(riskLabelKeys[command.risk])}
          </Badge>
          <Badge variant="secondary">{t(approvalLabelKeys[command.approvalState])}</Badge>
        </div>
      </div>
      <dl className="mt-4 grid gap-3 text-sm">
        <DetailRow label={t('command.executable')} value={command.executable} />
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
        <DetailRow label={t('command.cwd')} value={command.cwd} />
        <div className="grid gap-1">
          <dt className="text-muted-foreground text-xs">{t('command.environment')}</dt>
          <dd className="space-y-1 font-mono text-xs">
            {command.environment.map((entry) => (
              <div key={entry.name}>
                {entry.name}={entry.redacted ? '[REDACTED]' : entry.value}
              </div>
            ))}
          </dd>
        </div>
      </dl>
    </section>
  )
}

function formatArg(arg: string) {
  return /\s/.test(arg) ? JSON.stringify(arg) : arg
}

function DetailRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="grid gap-1">
      <dt className="text-muted-foreground text-xs">{label}</dt>
      <dd className="break-words font-mono text-xs">{value}</dd>
    </div>
  )
}
