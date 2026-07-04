import { describe, expect, it } from 'vitest'

import type { ConversationTurn } from '@/shared/tauri/commands'
import { assistantWork, permissionState } from '@/testing/conversation-worktree-builders'
import {
  selectComposerMode,
  selectPendingPermissions,
  selectTurnGroups,
  selectTurns,
} from './conversation-timeline-selectors'
import { createConversationTimelineState } from './conversation-timeline-store'

describe('conversation timeline selectors', () => {
  it('returns projected turns as the timeline model', () => {
    const state = stateWithTurns([turn({ id: 'turn:user-001' })])

    expect(selectTurns(state)).toEqual(state.pages[0].turns)
    expect(selectTurnGroups(state.pages[0].turns)).toEqual([
      { turnId: 'turn:user-001', turns: [state.pages[0].turns[0]] },
    ])
  })

  it('derives composer mode from assistant work status and nested request segments', () => {
    expect(selectComposerMode(stateWithTurns([turn({ assistantStatus: 'running' })]))).toEqual({
      kind: 'running-disabled',
      canCancel: true,
    })

    expect(
      selectComposerMode(stateWithTurns([turn({ segmentKind: 'clarificationRequest' })])),
    ).toEqual({ kind: 'clarification-reply', segmentId: 'segment:clarification:request-001' })

    expect(selectComposerMode(stateWithTurns([turn({ assistantStatus: 'failed' })]))).toEqual({
      kind: 'retry',
      turnId: 'turn:user-001',
    })
  })

  it('selects pending permissions nested under tool attempts', () => {
    const state = stateWithTurns([turn({ toolPermissionStatus: 'pending' })])

    expect(selectPendingPermissions(state)).toEqual([
      expect.objectContaining({
        conversationId: 'conversation-001',
        requestId: 'request-001',
        toolUseId: 'tool-use-001',
        turnId: 'turn:user-001',
      }),
    ])
  })

  it('selects pending permissions nested under agent activity segments', () => {
    const state = stateWithTurns([
      turn({ segmentKind: 'agentActivity', agentActivityPermissionStatus: 'pending' }),
    ])

    expect(selectPendingPermissions(state)).toEqual([
      expect.objectContaining({
        conversationId: 'conversation-001',
        requestId: 'request-agent-001',
        toolUseId: 'subagent-001',
        turnId: 'turn:user-001',
        toolAttempt: expect.objectContaining({
          toolUseId: 'subagent-001',
          toolName: 'Reviewer',
          status: 'waitingPermission',
        }),
      }),
    ])
  })
})

function stateWithTurns(turns: ConversationTurn[]) {
  return {
    ...createConversationTimelineState('conversation-001'),
    pages: [{ cursor: null, turns }],
  }
}

function turn(input: {
  agentActivityPermissionStatus?: 'pending' | 'approved'
  assistantStatus?: 'running' | 'complete' | 'failed' | 'cancelled'
  id?: string
  segmentKind?: 'clarificationRequest' | 'agentActivity'
  toolPermissionStatus?: 'pending' | 'approved'
}): ConversationTurn {
  return {
    id: input.id ?? 'turn:user-001',
    conversationId: 'conversation-001',
    position: 0,
    user: {
      id: 'user:user-001',
      messageId: 'user-001',
      body: 'Prompt',
      timestamp: '2026-06-17T00:00:00.000Z',
    },
    assistant: assistantWork({
      id: 'assistant:run-001',
      runId: 'run-001',
      status: input.assistantStatus ?? 'complete',
      segments:
        input.segmentKind === 'clarificationRequest'
          ? [
              {
                kind: 'clarificationRequest',
                id: 'segment:clarification:request-001',
                order: 0,
                requestId: 'request-001',
                prompt: 'Which target?',
              },
            ]
          : input.segmentKind === 'agentActivity'
            ? [
                {
                  kind: 'agentActivity',
                  id: 'segment:agent:subagent-001',
                  order: 0,
                  activityKind: 'subagent',
                  agentId: 'subagent-001',
                  role: 'Reviewer',
                  taskSummary: 'Review recent changes',
                  status: 'waitingPermission',
                  permission: permissionState({
                    id: 'permission:request-agent-001',
                    requestId: 'request-agent-001',
                    status: input.agentActivityPermissionStatus ?? 'approved',
                    reason: 'Review recent changes',
                  }),
                },
              ]
            : [
                {
                  kind: 'toolGroup',
                  id: 'segment:tools:tool-use-001',
                  order: 0,
                  attempts: [
                    {
                      id: 'tool:tool-use-001',
                      order: 0,
                      toolUseId: 'tool-use-001',
                      toolName: 'read_file',
                      status: 'waitingPermission',
                      permission: permissionState({
                        id: 'permission:request-001',
                        requestId: 'request-001',
                        toolUseId: 'tool-use-001',
                        status: input.toolPermissionStatus ?? 'approved',
                        reason: 'Read workspace file',
                      }),
                    },
                  ],
                },
              ],
    }),
  }
}
