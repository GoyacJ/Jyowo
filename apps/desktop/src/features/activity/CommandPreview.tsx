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

const riskLabels = {
  critical: 'Critical risk',
  high: 'High risk',
  low: 'Low risk',
  medium: 'Medium risk',
} satisfies Record<CommandRisk, string>

const approvalLabels = {
  approved: 'Approval approved',
  denied: 'Approval denied',
  pending: 'Approval pending',
} satisfies Record<CommandApprovalState, string>

export function CommandPreview({ command }: { command: CommandDetails }) {
  return (
    <section
      aria-labelledby="command-preview-title"
      className="rounded-md border border-border p-4"
    >
      <div className="flex items-center justify-between gap-3">
        <h3 className="font-medium" id="command-preview-title">
          Command
        </h3>
        <div className="flex items-center gap-2">
          <Badge variant={command.risk === 'critical' ? 'destructive' : 'outline'}>
            {riskLabels[command.risk]}
          </Badge>
          <Badge variant="secondary">{approvalLabels[command.approvalState]}</Badge>
        </div>
      </div>
      <dl className="mt-4 grid gap-3 text-sm">
        <DetailRow label="Executable" value={command.executable} />
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
        <DetailRow label="Cwd" value={command.cwd} />
        <div className="grid gap-1">
          <dt className="text-muted-foreground text-xs">Environment</dt>
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
