import { useTranslation } from 'react-i18next'

import type { AgentActivitySegment as AgentActivitySegmentType } from '@/shared/tauri/commands'
import { Button } from '@/shared/ui/button'

import { DecisionPanel } from './evidence/DecisionPanel'

export function AgentActivitySegmentView({
  conversationId,
  onPermissionResolve,
  parentRunId,
  segment,
  turnId,
}: {
  conversationId: string
  onPermissionResolve?: (request: {
    conversationId: string
    requestId: string
    decision: 'approve' | 'deny'
    confirmationText?: string
  }) => void
  parentRunId?: string
  segment: AgentActivitySegmentType
  turnId: string
}) {
  const { t } = useTranslation('conversation')
  const statusLabel = t(`timeline.agentActivity.status.${segment.status}`)
  const kindLabel = t(`timeline.agentActivity.kind.${segment.activityKind}`)

  return (
    <section
      className="rounded-md border border-border bg-muted/30 px-3 py-2"
      data-agent-activity-id={segment.agentId}
      data-agent-activity-status={segment.status}
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="font-medium text-sm">
          {kindLabel}: {segment.role}
        </div>
        <span className="text-muted-foreground text-xs">{statusLabel}</span>
      </div>
      <p className="mt-2 text-sm">{segment.taskSummary}</p>
      {segment.resultSummary && segment.status !== 'redacted' ? (
        <p className="mt-2 text-muted-foreground text-sm">{segment.resultSummary}</p>
      ) : null}
      {segment.status === 'redacted' ? (
        <p className="mt-2 text-muted-foreground text-xs">{t('timeline.agentActivity.redacted')}</p>
      ) : null}
      {segment.activityKind === 'agentTeam' && segment.team ? (
        <TeamActivityDetails segment={segment} />
      ) : null}
      {segment.activityKind === 'backgroundAgent' ? (
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <Button asChild size="sm" variant="outline">
            <a href={`/background-agents?backgroundAgentId=${encodeURIComponent(segment.agentId)}`}>
              {t('timeline.agentActivity.openBackgroundAgent')}
            </a>
          </Button>
        </div>
      ) : null}
      {segment.permission ? (
        <div className="mt-2">
          {segment.activityKind === 'backgroundAgent' ? (
            <div className="mb-2 flex flex-wrap gap-x-3 gap-y-1 text-muted-foreground text-xs">
              <span>{segment.agentId}</span>
              <span>{conversationId}</span>
              {parentRunId ? <span>{parentRunId}</span> : null}
            </div>
          ) : null}
          <DecisionPanel
            conversationId={conversationId}
            decision={{
              id: segment.permission.id,
              requestId: segment.permission.requestId,
              toolUseId: segment.agentId,
              status: segment.permission.status,
              operation: 'unknown',
              target: { kind: 'unknown', label: segment.role },
              riskLevel: 'medium',
              reason: segment.permission.summary ?? '',
              policy: { mode: 'default' },
              decisionOptions: [],
              dataExposure: {
                sendsWorkspaceData: false,
                sendsNetworkData: false,
                touchesPrivatePath: false,
                secretRisk: 'none',
              },
            }}
            onResolve={onPermissionResolve}
          />
        </div>
      ) : null}
    </section>
  )
}

function TeamActivityDetails({ segment }: { segment: AgentActivitySegmentType }) {
  const { t } = useTranslation('conversation')
  const team = segment.team

  if (!team) {
    return null
  }

  return (
    <div className="mt-3 space-y-2 text-xs">
      <div className="flex flex-wrap gap-2 text-muted-foreground">
        <span>
          {t('timeline.agentActivity.team.topology')}: {team.topology}
        </span>
        <span>
          {t('timeline.agentActivity.team.mailbox')}: {team.mailboxCount}
        </span>
      </div>
      {team.lead ? (
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-muted-foreground">{t('timeline.agentActivity.team.lead')}</span>
          <TeamMemberPill member={team.lead} />
        </div>
      ) : null}
      {team.members && team.members.length > 0 ? (
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-muted-foreground">{t('timeline.agentActivity.team.members')}</span>
          {team.members.map((member) => (
            <TeamMemberPill key={member.agentId} member={member} />
          ))}
        </div>
      ) : null}
      <div>
        <div className="text-muted-foreground">{t('timeline.agentActivity.team.tasks')}</div>
        {team.currentTasks && team.currentTasks.length > 0 ? (
          <ul className="mt-1 space-y-1">
            {team.currentTasks.map((task) => (
              <li
                className="flex flex-wrap items-center gap-2 rounded border border-border bg-background px-2 py-1"
                key={task.id}
              >
                <span>{task.title}</span>
                <span className="text-muted-foreground">{task.status}</span>
                {task.assigneeProfileId ? (
                  <span className="text-muted-foreground">{task.assigneeProfileId}</span>
                ) : null}
              </li>
            ))}
          </ul>
        ) : (
          <p className="mt-1 text-muted-foreground">{t('timeline.agentActivity.team.noTasks')}</p>
        )}
      </div>
      {team.mailboxSummaries && team.mailboxSummaries.length > 0 ? (
        <div>
          <div className="text-muted-foreground">
            {t('timeline.agentActivity.team.mailboxSummaries')}
          </div>
          <ul className="mt-1 space-y-1">
            {team.mailboxSummaries.map((summary) => (
              <li className="rounded border border-border bg-background px-2 py-1" key={summary}>
                {summary}
              </li>
            ))}
          </ul>
        </div>
      ) : null}
    </div>
  )
}

function TeamMemberPill({
  member,
}: {
  member: NonNullable<NonNullable<AgentActivitySegmentType['team']>['members']>[number]
}) {
  const { t } = useTranslation('conversation')

  return (
    <span className="inline-flex items-center gap-1 rounded border border-border bg-background px-2 py-1">
      <span>{member.role}</span>
      <span className="text-muted-foreground">
        {t(`timeline.agentActivity.status.${member.status}`)}
      </span>
    </span>
  )
}
