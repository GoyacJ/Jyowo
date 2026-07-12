import { useTranslation } from 'react-i18next'
import type { SubagentProjection } from '@/generated/daemon-protocol'

export function SubagentsPanel({ subagents }: { subagents: SubagentProjection[] }) {
  const { t } = useTranslation('tasks')
  if (subagents.length === 0) return <Empty>{t('workbench.empty.agents')}</Empty>
  return (
    <ul className="divide-y divide-border/70">
      {subagents.map((agent) => (
        <li className="space-y-1 px-4 py-3" key={agent.delegationId}>
          <div className="flex items-center justify-between gap-3">
            <span className="truncate text-sm">{agent.summary ?? agent.childTaskId}</span>
            <span className="shrink-0 text-muted-foreground text-xs">
              {agent.state.replace('_', ' ')}
            </span>
          </div>
          <p className="truncate font-mono text-[11px] text-muted-foreground">
            {agent.childTaskId}
          </p>
        </li>
      ))}
    </ul>
  )
}

function Empty({ children }: { children: React.ReactNode }) {
  return (
    <p className="flex min-h-48 items-center justify-center px-6 text-center text-muted-foreground text-sm">
      {children}
    </p>
  )
}
