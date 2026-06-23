import { QueryClient } from '@tanstack/react-query'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { conversationQueryKeys } from '@/features/conversation/use-conversation'
import { uiStore } from '@/shared/state/ui-store'
import { onProjectWorkspaceChanged } from './reset-workspace-scope'

describe('onProjectWorkspaceChanged', () => {
  beforeEach(() => {
    uiStore.getState().clearActiveRun()
    uiStore.getState().clearTimelineScrollRequest()
  })

  it('clears run state, conversation cache, and navigates to welcome', async () => {
    const queryClient = new QueryClient()
    const navigate = vi.fn(async () => undefined)
    const workspacePath = '/Users/goya/Repo/Git/Jyowo'

    uiStore.getState().setActiveRun({
      conversationId: 'conversation-runtime-001',
      runId: 'run-001',
    })
    queryClient.setQueryData(conversationQueryKeys.list(workspacePath), {
      conversations: [{ id: 'conversation-runtime-001' }],
    })
    queryClient.setQueryData(
      conversationQueryKeys.detail(workspacePath, 'conversation-runtime-001'),
      { conversation: { id: 'conversation-runtime-001' } },
    )

    await onProjectWorkspaceChanged(queryClient, navigate)

    expect(uiStore.getState().activeRunId).toBeUndefined()
    expect(uiStore.getState().activeRunConversationId).toBeUndefined()
    expect(queryClient.getQueryData(conversationQueryKeys.list(workspacePath))).toBeUndefined()
    expect(
      queryClient.getQueryData(
        conversationQueryKeys.detail(workspacePath, 'conversation-runtime-001'),
      ),
    ).toBeUndefined()
    expect(navigate).toHaveBeenCalledWith({ replace: true, search: {}, to: '/' })
  })
})
