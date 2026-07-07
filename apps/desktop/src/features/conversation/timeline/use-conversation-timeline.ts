import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useUiStore } from '@/shared/state/ui-store'
import type {
  PageConversationWorktreeResponse,
  ResolvePermissionRequest,
} from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'
import type { ComposerSubmitPayload } from '../Composer'
import {
  type ConversationRecord,
  conversationQueryKeys,
  useConversation,
} from '../use-conversation'
import type { ConversationTimelineAction } from './conversation-timeline-actions'
import {
  selectComposerMode,
  selectPendingPermissions,
  selectShouldPollFallback,
  selectTurns,
} from './conversation-timeline-selectors'
import type { ConversationTimelineState } from './conversation-timeline-store'
import {
  conversationTimelineRootReducerFromAction,
  createConversationTimelineRoot,
  getConversationTimelineState,
} from './conversation-timeline-store'
import { useConversationEventStream } from './use-conversation-event-stream'

const worktreeRefetchThrottleMs = 500

export function useConversationTimeline({ conversationId }: { conversationId?: string }) {
  const { t } = useTranslation('conversation')
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const setActiveRun = useUiStore((state) => state.setActiveRun)
  const clearActiveRun = useUiStore((state) => state.clearActiveRun)
  const activeRunConversationId = useUiStore((state) => state.activeRunConversationId)
  const activeRunId = useUiStore((state) => state.activeRunId)
  const conversation = useConversation({ conversationId })
  const workspaceKey = conversation.workspacePath ?? 'none'
  const renderedConversation = useMemo(
    () =>
      conversation.conversation ??
      createDraftConversation(conversation.selectedConversationId, t('draftTitle')),
    [conversation.conversation, conversation.selectedConversationId, t],
  )
  const [root, dispatchRoot] = useReducer(
    conversationTimelineRootReducerFromAction,
    undefined,
    () => createConversationTimelineRoot(),
  )
  const [loadingEarlier, setLoadingEarlier] = useState(false)
  const [loadingLater, setLoadingLater] = useState(false)
  const hadActiveRunRef = useRef(false)
  const refreshTimeoutRef = useRef<number | null>(null)
  const refreshTimeoutConversationRef = useRef<string | null>(null)
  const renderedConversationId = renderedConversation?.id ?? null
  const activeConversationId =
    renderedConversationId ?? conversation.selectedConversationId ?? 'conversation-pending'
  const displayState = getConversationTimelineState(root, activeConversationId)
  const worktreeQueryKey = useMemo(
    () => ['conversation-worktree', workspaceKey, renderedConversationId] as const,
    [renderedConversationId, workspaceKey],
  )

  const dispatch = useCallback(
    (action: ConversationTimelineAction) => {
      dispatchRoot({ conversationId: activeConversationId, action })
    },
    [activeConversationId],
  )

  const worktreeQuery = useQuery({
    enabled: Boolean(renderedConversationId),
    queryFn: () => {
      if (!renderedConversationId) {
        throw new Error('conversationId is required for worktree paging')
      }
      return commandClient.pageConversationWorktree({
        conversationId: renderedConversationId,
        direction: 'before',
        limit: 100,
      })
    },
    queryKey: worktreeQueryKey,
  })

  useEffect(() => {
    if (!worktreeQuery.data) {
      return
    }
    dispatch({
      type: 'hydrateInitialPage',
      turns: worktreeQuery.data.turns,
      pageCursor: worktreeQuery.data.pageCursor ?? null,
      eventCursor: worktreeQuery.data.eventCursor ?? null,
      hasMoreBefore: worktreeQuery.data.hasMoreBefore,
      hasMoreAfter: worktreeQuery.data.hasMoreAfter,
      gap: worktreeQuery.data.gap,
    })
  }, [dispatch, worktreeQuery.data])

  useConversationEventStream({
    conversationId: renderedConversationId,
    cursor: displayState.eventCursor,
    dispatch,
  })

  useEffect(() => {
    if (!renderedConversationId || displayState.immediateRefreshRequests === 0) {
      return
    }
    void queryClient.invalidateQueries({ queryKey: worktreeQueryKey })
  }, [displayState.immediateRefreshRequests, queryClient, renderedConversationId, worktreeQueryKey])

  useEffect(() => {
    if (!renderedConversationId || displayState.refreshRequests === 0) {
      return
    }
    if (
      refreshTimeoutRef.current !== null &&
      refreshTimeoutConversationRef.current !== renderedConversationId
    ) {
      window.clearTimeout(refreshTimeoutRef.current)
      refreshTimeoutRef.current = null
      refreshTimeoutConversationRef.current = null
    }
    if (refreshTimeoutRef.current !== null) {
      return
    }

    refreshTimeoutConversationRef.current = renderedConversationId
    refreshTimeoutRef.current = window.setTimeout(() => {
      refreshTimeoutRef.current = null
      refreshTimeoutConversationRef.current = null
      void queryClient.invalidateQueries({ queryKey: worktreeQueryKey })
    }, worktreeRefetchThrottleMs)
  }, [displayState.refreshRequests, queryClient, renderedConversationId, worktreeQueryKey])

  useEffect(() => {
    return () => {
      if (refreshTimeoutRef.current !== null) {
        window.clearTimeout(refreshTimeoutRef.current)
        refreshTimeoutRef.current = null
        refreshTimeoutConversationRef.current = null
      }
    }
  }, [])

  useEffect(() => {
    if (!renderedConversationId) {
      hadActiveRunRef.current = false
      return
    }

    const worktreeActiveRunIds = activeRunIdsFromTurns(worktreeQuery.data?.turns ?? [])
    if (displayState.activeRunIds.length > 0 || worktreeActiveRunIds.length > 0) {
      hadActiveRunRef.current = true
      return
    }

    if (!worktreeQuery.data) {
      return
    }

    const hasUiActiveRunForConversation =
      Boolean(activeRunId) && activeRunConversationId === renderedConversationId

    if (!hadActiveRunRef.current && !hasUiActiveRunForConversation) {
      return
    }

    hadActiveRunRef.current = false
    clearActiveRun(renderedConversationId)
    void queryClient.invalidateQueries({
      queryKey: conversationQueryKeys.detail(workspaceKey, renderedConversationId),
    })
    void queryClient.invalidateQueries({ queryKey: conversationQueryKeys.list(workspaceKey) })
  }, [
    clearActiveRun,
    activeRunConversationId,
    activeRunId,
    displayState.activeRunIds.length,
    queryClient,
    renderedConversationId,
    worktreeQuery.data,
    workspaceKey,
  ])

  const submitMutation = useMutation({
    mutationFn: async (draft: ComposerSubmitPayload) => {
      if (!renderedConversation?.id) {
        throw new Error('No conversation selected')
      }

      const clientMessageId = createClientMessageId()
      dispatch({
        type: 'localSubmit',
        clientMessageId,
        draft,
        at: new Date().toISOString(),
      })

      try {
        const response = await commandClient.startRun({
          ...draft,
          conversationId: renderedConversation.id,
          clientMessageId,
        })
        dispatch({ type: 'commandAccepted', clientMessageId, runId: response.runId })
        setActiveRun({ conversationId: renderedConversation.id, runId: response.runId })
        void queryClient.invalidateQueries({ queryKey: worktreeQueryKey })
      } catch (error) {
        dispatch({
          type: 'commandFailed',
          clientMessageId,
          errorMessage: error instanceof Error ? error.message : 'Run failed',
        })
        throw error
      }
    },
  })

  const permissionMutation = useMutation({
    mutationFn: async (request: ResolvePermissionRequest) => {
      dispatch({
        type: 'permissionSubmitting',
        requestId: request.requestId,
        decision: request.decision,
      })
      try {
        await commandClient.resolvePermission(request)
        void queryClient.invalidateQueries({ queryKey: worktreeQueryKey })
      } catch (error) {
        dispatch({
          type: 'permissionSubmitFailed',
          requestId: request.requestId,
          errorMessage: error instanceof Error ? error.message : 'Permission update failed',
        })
        throw error
      }
    },
  })

  const cancelMutation = useMutation({
    mutationFn: async () => {
      const runId = displayState.activeRunIds.at(-1)
      if (!runId) {
        throw new Error('No active run to cancel')
      }

      await commandClient.cancelRun(runId)
      void queryClient.invalidateQueries({ queryKey: worktreeQueryKey })
    },
  })

  const loadEarlier = useCallback(async () => {
    if (!renderedConversationId || !displayState.hasMoreBefore) return
    setLoadingEarlier(true)
    try {
      const firstPage = displayState.pages[0]
      const firstTurn = firstPage?.turns[0]
      const page = await commandClient.pageConversationWorktree({
        conversationId: renderedConversationId,
        direction: 'before',
        pageCursor: firstTurn
          ? {
              turnId: firstTurn.id,
              position: firstTurn.position,
            }
          : (firstPage?.cursor ?? undefined),
        limit: 50,
      })
      queryClient.setQueryData<PageConversationWorktreeResponse>(worktreeQueryKey, (current) =>
        current ? mergeWorktreePage(current, page, 'before') : page,
      )
      dispatch({
        type: 'prependPage',
        turns: page.turns,
        pageCursor: page.pageCursor ?? null,
        hasMoreBefore: page.hasMoreBefore,
      })
    } finally {
      setLoadingEarlier(false)
    }
  }, [
    renderedConversationId,
    displayState.hasMoreBefore,
    displayState.pages,
    commandClient,
    dispatch,
    queryClient,
    worktreeQueryKey,
  ])

  const loadLater = useCallback(async () => {
    if (!renderedConversationId || !displayState.hasMoreAfter) return
    setLoadingLater(true)
    try {
      const lastPage = displayState.pages[displayState.pages.length - 1]
      const lastTurn = lastPage?.turns.at(-1)
      const page = await commandClient.pageConversationWorktree({
        conversationId: renderedConversationId,
        direction: 'after',
        pageCursor: lastTurn
          ? {
              turnId: lastTurn.id,
              position: lastTurn.position,
            }
          : (lastPage?.cursor ?? undefined),
        limit: 50,
      })
      queryClient.setQueryData<PageConversationWorktreeResponse>(worktreeQueryKey, (current) =>
        current ? mergeWorktreePage(current, page, 'after') : page,
      )
      dispatch({
        type: 'appendPage',
        turns: page.turns,
        pageCursor: page.pageCursor ?? null,
        eventCursor: page.eventCursor ?? null,
        hasMoreAfter: page.hasMoreAfter,
      })
    } finally {
      setLoadingLater(false)
    }
  }, [
    renderedConversationId,
    displayState.hasMoreAfter,
    displayState.pages,
    commandClient,
    dispatch,
    queryClient,
    worktreeQueryKey,
  ])

  const retryGap = useCallback(() => {
    dispatch({ type: 'retryGap' })
    void queryClient.invalidateQueries({ queryKey: worktreeQueryKey })
  }, [dispatch, queryClient, worktreeQueryKey])

  return {
    turns: selectTurns(displayState),
    composerMode: submitMutation.isPending
      ? { kind: 'submitting' as const }
      : selectComposerMode(displayState),
    conversation: renderedConversation,
    error: conversation.error ?? worktreeQuery.error,
    getTimelineState: (targetConversationId: string): ConversationTimelineState =>
      getConversationTimelineState(root, targetConversationId),
    isEmpty: conversation.isEmpty,
    isLoading: conversation.isLoading || worktreeQuery.isLoading,
    isCancelling: cancelMutation.isPending,
    isSubmitting: submitMutation.isPending,
    pendingToolPermissions: selectPendingPermissions(displayState),
    shouldPollFallback: selectShouldPollFallback(displayState),
    loadEarlier,
    loadLater,
    loadingEarlier,
    loadingLater,
    hasMoreBefore: displayState.hasMoreBefore,
    hasMoreAfter: displayState.hasMoreAfter,
    retryGap,
    gapMarkers: displayState.gapMarkers,
    cancelActiveRun: cancelMutation.mutateAsync,
    cancelError: cancelMutation.error,
    submitError: submitMutation.error,
    submitPrompt: submitMutation.mutateAsync,
    resolvePermission: permissionMutation.mutateAsync,
    state: displayState,
    workspacePath: conversation.workspacePath,
    workspacePathReady: conversation.workspacePathReady,
  }
}

function createDraftConversation(
  conversationId: string | undefined,
  title: string,
): ConversationRecord | null {
  if (!conversationId) {
    return null
  }

  return {
    id: conversationId,
    messages: [],
    modelConfigId: null,
    title,
    updatedAt: new Date(0).toISOString(),
  }
}

function activeRunIdsFromTurns(turns: Array<{ assistant?: { runId: string; status: string } }>) {
  return turns.flatMap((turn) =>
    turn.assistant?.status === 'running' ? [turn.assistant.runId] : [],
  )
}

function mergeWorktreePage(
  current: PageConversationWorktreeResponse,
  page: PageConversationWorktreeResponse,
  direction: 'before' | 'after',
): PageConversationWorktreeResponse {
  const turns =
    direction === 'before' ? [...page.turns, ...current.turns] : [...current.turns, ...page.turns]

  return {
    ...current,
    turns: dedupeTurns(turns),
    pageCursor:
      direction === 'before' ? (page.pageCursor ?? current.pageCursor) : current.pageCursor,
    eventCursor:
      direction === 'after' ? (page.eventCursor ?? current.eventCursor) : current.eventCursor,
    hasMoreBefore: direction === 'before' ? page.hasMoreBefore : current.hasMoreBefore,
    hasMoreAfter: direction === 'after' ? page.hasMoreAfter : current.hasMoreAfter,
    gap: current.gap || page.gap,
  }
}

function dedupeTurns(turns: PageConversationWorktreeResponse['turns']) {
  const seen = new Set<string>()
  const deduped: PageConversationWorktreeResponse['turns'] = []

  for (const turn of turns) {
    if (seen.has(turn.id)) {
      continue
    }
    seen.add(turn.id)
    deduped.push(turn)
  }

  return deduped
}

function createClientMessageId() {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
    return crypto.randomUUID()
  }

  return '10000000-1000-4000-8000-100000000000'.replace(/[018]/g, (value) =>
    (Number(value) ^ ((Math.random() * 16) >> (Number(value) / 4))).toString(16),
  )
}
