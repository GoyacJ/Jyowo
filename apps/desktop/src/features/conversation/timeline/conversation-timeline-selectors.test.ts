import { describe, expect, it } from 'vitest'

import type { ConversationTurn } from '@/shared/tauri/commands'
import {
  selectComposerMode,
  selectPendingPermissions,
  selectTurnGroups,
  selectTurns,
} from './conversation-timeline-selectors'
import { createConversationTimelineState } from './conversation-timeline-store'

describe('conversation timeline selectors', () => {
  it('returns projected turns as the timeline model', () => {
    const state = {
      ...createConversationTimelineState('conversation-001'),
      turns: [turn({ id: 'turn:user-001' })],
    }

    expect(selectTurns(state)).toEqual(state.turns)
    expect(selectTurnGroups(state.turns)).toEqual([
      { turnId: 'turn:user-001', turns: [state.turns[0]] },
    ])
  })

  it('derives composer mode from assistant work status and nested request segments', () => {
    expect(
      selectComposerMode({
        ...createConversationTimelineState('conversation-001'),
        turns: [turn({ assistantStatus: 'running' })],
      }),
    ).toEqual({ kind: 'running-disabled', canCancel: true })

    expect(
      selectComposerMode({
        ...createConversationTimelineState('conversation-001'),
        turns: [turn({ segmentKind: 'clarificationRequest' })],
      }),
    ).toEqual({ kind: 'clarification-reply', blockId: 'segment:clarification:request-001' })

    expect(
      selectComposerMode({
        ...createConversationTimelineState('conversation-001'),
        turns: [turn({ assistantStatus: 'failed' })],
      }),
    ).toEqual({ kind: 'retry', turnId: 'turn:user-001' })
  })

  it('selects pending permissions nested under tool attempts', () => {
    const state = {
      ...createConversationTimelineState('conversation-001'),
      turns: [turn({ toolPermissionStatus: 'pending' })],
    }

    expect(selectPendingPermissions(state)).toEqual([
      expect.objectContaining({
        conversationId: 'conversation-001',
        requestId: 'request-001',
        toolUseId: 'tool-use-001',
        turnId: 'turn:user-001',
      }),
    ])
  })
})

function turn(input: {
  assistantStatus?: 'running' | 'complete' | 'failed' | 'cancelled'
  id?: string
  segmentKind?: 'clarificationRequest'
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
    assistant: {
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
                    permission: {
                      id: 'permission:request-001',
                      requestId: 'request-001',
                      toolUseId: 'tool-use-001',
                      status: input.toolPermissionStatus ?? 'approved',
                    },
                  },
                ],
              },
            ],
    },
  }
}
