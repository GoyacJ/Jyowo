import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useCallback, useEffect, useMemo, useReducer, useRef } from 'react'
import { useTranslation } from 'react-i18next'

import { useUiStore } from '@/shared/state/ui-store'
import { useCommandClient } from '@/shared/tauri/react'
import type { ComposerSubmitPayload } from '../Composer'
import { type ConversationRecord, useConversation } from '../use-conversation'
import type { ConversationTimelineAction } from './conversation-timeline-actions'
import type { ConversationTimelineState } from './conversation-timeline-reducer'
import {
  selectBlocks,
  selectComposerMode,
  selectPendingPermissionBlocks,
  selectShouldPollFallback,
} from './conversation-timeline-selectors'
import {
  conversationTimelineRootReducerFromAction,
  createConversationTimelineRoot,
  getConversationTimelineState,
} from './conversation-timeline-store'
import { useConversationEventStream } from './use-conversation-event-stream'

const artifactRefreshIntervalMs = 2000
const gapRecoveryPageLimit = 200

export function useConversationTimeline({ conversationId }: { conversationId?: string }) {
  const { t } = useTranslation('conversation')
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const setActiveRun = useUiStore((state) => state.setActiveRun)
  const clearActiveRun = useUiStore((state) => state.clearActiveRun)
  const conversation = useConversation({ conversationId })
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
  const hadActiveRunRef = useRef(false)
  const gapRecoveryKeyRef = useRef<string | null>(null)
  const renderedConversationId = renderedConversation?.id ?? null
  const activeConversationId =
    renderedConversationId ?? conversation.selectedConversationId ?? 'conversation-pending'
  const displayState = getConversationTimelineState(root, activeConversationId)

  const dispatch = useCallback(
    (action: ConversationTimelineAction) => {
      dispatchRoot({ conversationId: activeConversationId, action })
    },
    [activeConversationId],
  )

  useEffect(() => {
    if (!renderedConversation) {
      return
    }
    dispatch({ type: 'hydrateSnapshot', snapshot: renderedConversation })
  }, [dispatch, renderedConversation])

  useEffect(() => {
    if (!renderedConversation || !conversation.conversation) {
      return
    }
    dispatch({ type: 'snapshotReconciled', snapshot: conversation.conversation })
  }, [conversation.conversation, dispatch, renderedConversation])

  useConversationEventStream({
    conversationId: renderedConversationId,
    cursor: displayState.cursor,
    dispatch,
  })

  useEffect(() => {
    if (!renderedConversationId || !displayState.hasGap) {
      gapRecoveryKeyRef.current = null
      return
    }

    const recoveryKey = [
      renderedConversationId,
      displayState.cursor?.eventId ?? 'start',
      displayState.cursor?.conversationSequence ?? 0,
      displayState.gapRecoverySequence ?? 'unknown',
    ].join(':')
    if (gapRecoveryKeyRef.current === recoveryKey) {
      return
    }
    gapRecoveryKeyRef.current = recoveryKey

    let cancelled = false

    void commandClient
      .pageConversationTimeline({
        conversationId: renderedConversationId,
        limit: gapRecoveryPageLimit,
        ...(displayState.cursor ? { afterCursor: displayState.cursor } : {}),
      })
      .then((page) => {
        if (cancelled) {
          return
        }
        dispatch({
          type: 'applyEvents',
          events: page.events,
          cursor: page.cursor ?? null,
        })
        if (page.gap) {
          dispatch({
            type: 'markGap',
            afterCursor: page.cursor ?? displayState.cursor ?? undefined,
          })
          void queryClient.invalidateQueries({
            queryKey: ['conversation', 'detail', renderedConversationId],
          })
          void queryClient.invalidateQueries({ queryKey: ['conversation', 'list'] })
        }
      })
      .catch(() => {
        if (cancelled) {
          return
        }
        void queryClient.invalidateQueries({
          queryKey: ['conversation', 'detail', renderedConversationId],
        })
        void queryClient.invalidateQueries({ queryKey: ['conversation', 'list'] })
      })

    return () => {
      cancelled = true
    }
  }, [
    commandClient,
    dispatch,
    displayState.cursor,
    displayState.gapRecoverySequence,
    displayState.hasGap,
    queryClient,
    renderedConversationId,
  ])

  useEffect(() => {
    if (!renderedConversationId) {
      hadActiveRunRef.current = false
      return
    }

    if (displayState.activeRunIds.length > 0) {
      hadActiveRunRef.current = true
      return
    }

    if (!hadActiveRunRef.current) {
      return
    }

    hadActiveRunRef.current = false
    clearActiveRun(renderedConversationId)
    void queryClient.invalidateQueries({
      queryKey: ['conversation', 'detail', renderedConversationId],
    })
    void queryClient.invalidateQueries({ queryKey: ['conversation', 'list'] })
  }, [clearActiveRun, displayState.activeRunIds.length, queryClient, renderedConversationId])

  const shouldPollFallback = selectShouldPollFallback(displayState)
  const artifactsQuery = useQuery({
    enabled: Boolean(renderedConversation?.id),
    queryFn: () => {
      if (!renderedConversation?.id) {
        throw new Error('conversationId is required for artifact listing')
      }
      return commandClient.listArtifacts({ conversationId: renderedConversation.id })
    },
    queryKey: ['conversation-timeline-artifacts', renderedConversation?.id],
    refetchInterval:
      shouldPollFallback || displayState.activeRunIds.length > 0
        ? artifactRefreshIntervalMs
        : false,
  })

  useEffect(() => {
    if (!artifactsQuery.data) {
      return
    }
    dispatch({ type: 'applyArtifacts', artifacts: artifactsQuery.data.artifacts })
  }, [artifactsQuery.data, dispatch])

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
    mutationFn: async (request: {
      conversationId: string
      requestId: string
      decision: 'approve' | 'deny'
    }) => {
      dispatch({
        type: 'permissionSubmitting',
        requestId: request.requestId,
        decision: request.decision,
      })
      try {
        await commandClient.resolvePermission(request)
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

  return {
    blocks: selectBlocks(displayState),
    composerMode: submitMutation.isPending
      ? { kind: 'submitting' as const }
      : selectComposerMode(displayState),
    conversation: renderedConversation,
    error: conversation.error,
    getTimelineState: (targetConversationId: string): ConversationTimelineState =>
      getConversationTimelineState(root, targetConversationId),
    isEmpty: conversation.isEmpty,
    isLoading: conversation.isLoading,
    isSubmitting: submitMutation.isPending,
    pendingPermissionBlocks: selectPendingPermissionBlocks(displayState),
    submitError: submitMutation.error,
    submitPrompt: submitMutation.mutateAsync,
    resolvePermission: permissionMutation.mutateAsync,
    state: displayState,
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

function createClientMessageId() {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
    return crypto.randomUUID()
  }

  return '10000000-1000-4000-8000-100000000000'.replace(/[018]/g, (value) =>
    (Number(value) ^ ((Math.random() * 16) >> (Number(value) / 4))).toString(16),
  )
}
