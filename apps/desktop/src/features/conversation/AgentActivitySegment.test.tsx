import '@testing-library/jest-dom/vitest'

import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { appI18n } from '@/shared/i18n/i18n'
import type { AgentActivitySegment } from '@/shared/tauri/commands'
import { permissionState } from '@/testing/conversation-worktree-builders'

import { AgentActivitySegmentView } from './AgentActivitySegment'

const baseSegment: AgentActivitySegment = {
  kind: 'agentActivity',
  id: 'segment:agent:subagent-1',
  order: 0,
  activityKind: 'subagent',
  agentId: 'subagent-1',
  role: 'Reviewer',
  taskSummary: 'Review recent changes',
  status: 'running',
}

const baseTeamDetails: NonNullable<AgentActivitySegment['team']> = {
  topology: 'coordinator_worker',
  lead: {
    agentId: 'agent-lead',
    role: 'Lead',
    status: 'running',
  },
  members: [
    {
      agentId: 'agent-lead',
      role: 'Lead',
      status: 'running',
    },
    {
      agentId: 'agent-worker',
      role: 'Worker',
      status: 'running',
    },
  ],
  currentTasks: [
    {
      id: 'task-1',
      title: 'Audit composer payload',
      status: 'running',
      assigneeProfileId: 'lead',
    },
  ],
  mailboxCount: 1,
  mailboxSummaries: ['Routed message message-1 to 1 member.'],
}

const baseTeamSegment: AgentActivitySegment = {
  kind: 'agentActivity',
  id: 'segment:agent-team:team-1',
  order: 0,
  activityKind: 'agentTeam',
  agentId: 'team-1',
  role: 'Migration team',
  taskSummary: 'Coordinate the migration',
  status: 'running',
  team: baseTeamDetails,
}

describe('AgentActivitySegmentView', () => {
  it('renders running subagent activity with role and task summary', () => {
    render(
      <AgentActivitySegmentView
        conversationId="conversation-1"
        segment={baseSegment}
        turnId="turn-1"
      />,
    )

    expect(screen.getByText(/Reviewer/)).toBeInTheDocument()
    expect(screen.getByText('Review recent changes')).toBeInTheDocument()
    expect(
      screen.getByText(appI18n.t('conversation:timeline.agentActivity.status.running')),
    ).toBeInTheDocument()
  })

  it('renders completed result summary', () => {
    render(
      <AgentActivitySegmentView
        conversationId="conversation-1"
        segment={{
          ...baseSegment,
          status: 'completed',
          resultSummary: 'No blocking issues found.',
        }}
        turnId="turn-1"
      />,
    )

    expect(screen.getByText('No blocking issues found.')).toBeInTheDocument()
  })

  it('renders permission panel when waiting for permission', () => {
    render(
      <AgentActivitySegmentView
        conversationId="conversation-1"
        segment={{
          ...baseSegment,
          status: 'waitingPermission',
          permission: permissionState({
            id: 'permission:req-1',
            requestId: 'req-1',
            status: 'pending',
            reason: 'Needs approval to continue.',
          }),
        }}
        turnId="turn-1"
      />,
    )

    expect(screen.getByText('Needs approval to continue.')).toBeInTheDocument()
    expect(
      screen.getByRole('button', { name: appI18n.t('conversation:timeline.approve') }),
    ).toBeInTheDocument()
  })

  it('links background agent activity to the background route and shows permission context', () => {
    render(
      <AgentActivitySegmentView
        conversationId="conversation-1"
        parentRunId="run-1"
        segment={{
          ...baseSegment,
          activityKind: 'backgroundAgent',
          agentId: 'bg-agent-1',
          role: 'Background worker',
          status: 'waitingPermission',
          permission: permissionState({
            id: 'permission:req-1',
            requestId: 'req-1',
            status: 'pending',
            reason: 'Needs approval to continue.',
          }),
        }}
        turnId="turn-1"
      />,
    )

    expect(
      screen.getByRole('link', {
        name: appI18n.t('conversation:timeline.agentActivity.openBackgroundAgent'),
      }),
    ).toHaveAttribute('href', '/background-agents?backgroundAgentId=bg-agent-1')
    expect(screen.getByText('bg-agent-1')).toBeInTheDocument()
    expect(screen.getByText('conversation-1')).toBeInTheDocument()
    expect(screen.getByText('run-1')).toBeInTheDocument()
  })

  it('renders redacted state without leaking withheld content', () => {
    render(
      <AgentActivitySegmentView
        conversationId="conversation-1"
        segment={{
          ...baseSegment,
          status: 'redacted',
          resultSummary: 'Subagent result withheld from conversation timeline.',
        }}
        turnId="turn-1"
      />,
    )

    expect(
      screen.getByText(appI18n.t('conversation:timeline.agentActivity.redacted')),
    ).toBeInTheDocument()
  })

  it.each([
    ['loading', 'conversation:timeline.agentActivity.status.loading'],
    ['failed', 'conversation:timeline.agentActivity.status.failed'],
    ['cancelled', 'conversation:timeline.agentActivity.status.cancelled'],
  ] as const)('renders %s status label from backend projection', (status, labelKey) => {
    render(
      <AgentActivitySegmentView
        conversationId="conversation-1"
        segment={{ ...baseSegment, status }}
        turnId="turn-1"
      />,
    )

    expect(screen.getByText(appI18n.t(labelKey))).toBeInTheDocument()
  })

  it('renders team topology, lead, members, tasks, and safe mailbox summaries', () => {
    render(
      <AgentActivitySegmentView
        conversationId="conversation-1"
        segment={baseTeamSegment}
        turnId="turn-1"
      />,
    )

    expect(screen.getByText(/coordinator_worker/)).toBeInTheDocument()
    expect(screen.getAllByText('Lead').length).toBeGreaterThan(0)
    expect(screen.getByText('Worker')).toBeInTheDocument()
    expect(screen.getByText('Audit composer payload')).toBeInTheDocument()
    expect(screen.getByText('Routed message message-1 to 1 member.')).toBeInTheDocument()
    expect(screen.queryByText(/secret raw payload/i)).not.toBeInTheDocument()
  })

  it('renders team empty task state', () => {
    render(
      <AgentActivitySegmentView
        conversationId="conversation-1"
        segment={{
          ...baseTeamSegment,
          team: {
            ...baseTeamDetails,
            currentTasks: [],
            mailboxCount: 0,
            mailboxSummaries: [],
          },
        }}
        turnId="turn-1"
      />,
    )

    expect(
      screen.getByText(appI18n.t('conversation:timeline.agentActivity.team.noTasks')),
    ).toBeInTheDocument()
  })

  it.each([
    ['failed', 'Failed member', 'conversation:timeline.agentActivity.status.failed'],
    ['cancelled', 'Cancelled member', 'conversation:timeline.agentActivity.status.cancelled'],
    ['completed', 'Completed member', 'conversation:timeline.agentActivity.status.completed'],
  ] as const)('renders %s team member state', (status, role, labelKey) => {
    render(
      <AgentActivitySegmentView
        conversationId="conversation-1"
        segment={{
          ...baseTeamSegment,
          status,
          team: {
            ...baseTeamDetails,
            members: [
              {
                agentId: 'agent-state',
                role,
                status,
              },
            ],
          },
        }}
        turnId="turn-1"
      />,
    )

    expect(screen.getByText(role)).toBeInTheDocument()
    expect(screen.getAllByText(appI18n.t(labelKey)).length).toBeGreaterThan(0)
  })
})
