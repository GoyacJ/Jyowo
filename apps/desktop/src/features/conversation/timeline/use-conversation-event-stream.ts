import { useCallback, useEffect, useMemo, useRef } from 'react'
import type { ConversationCursor } from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'
import type { ConversationTimelineAction } from './conversation-timeline-actions'
import { createConversationTimelineSource } from './conversation-timeline-source'

export function useConversationEventStream({
  conversationId,
  cursor,
  dispatch,
  resetKey = 0,
}: {
  conversationId: string | null
  cursor: ConversationCursor | null
  dispatch: (action: ConversationTimelineAction) => void
  resetKey?: number
}) {
  const commandClient = useCommandClient()
  const source = useMemo(() => createConversationTimelineSource(commandClient), [commandClient])
  const cursorRef = useRef(cursor)
  const streamDispatch = useAnimationFrameTimelineDispatch(dispatch)

  useEffect(() => {
    cursorRef.current = cursor
  }, [cursor])

  useEffect(() => {
    if (!conversationId) {
      return
    }

    let cleanup: (() => Promise<void>) | undefined
    let cancelled = false

    void source
      .subscribe(conversationId, cursorRef.current, streamDispatch.dispatch)
      .then((unsub) => {
        if (cancelled) {
          void unsub()
          return
        }
        cleanup = unsub
      })

    return () => {
      cancelled = true
      streamDispatch.cancel()
      void cleanup?.()
    }
  }, [conversationId, resetKey, source, streamDispatch])
}

export function coalesceTimelineActions(
  actions: ConversationTimelineAction[],
): ConversationTimelineAction[] {
  const coalesced: ConversationTimelineAction[] = []

  for (const action of actions) {
    const previous = coalesced.at(-1)
    if (
      previous?.type === 'worktreeRefreshRequested' &&
      action.type === 'worktreeRefreshRequested'
    ) {
      coalesced[coalesced.length - 1] = {
        type: 'worktreeRefreshRequested',
        immediate: previous.immediate || action.immediate,
      }
      continue
    }

    coalesced.push(action)
  }

  return coalesced
}

function useAnimationFrameTimelineDispatch(dispatch: (action: ConversationTimelineAction) => void) {
  const dispatchRef = useRef(dispatch)
  const queuedActionsRef = useRef<ConversationTimelineAction[]>([])
  const frameRef = useRef<number | null>(null)

  useEffect(() => {
    dispatchRef.current = dispatch
  }, [dispatch])

  const flush = useCallback(() => {
    frameRef.current = null
    const queuedActions = queuedActionsRef.current
    queuedActionsRef.current = []

    for (const action of coalesceTimelineActions(queuedActions)) {
      dispatchRef.current(action)
    }
  }, [])

  const cancel = useCallback(() => {
    if (frameRef.current !== null) {
      cancelAnimationFrameSafe(frameRef.current)
      frameRef.current = null
    }
    if (queuedActionsRef.current.length > 0) {
      flush()
    }
  }, [flush])

  const batchedDispatch = useCallback(
    (action: ConversationTimelineAction) => {
      queuedActionsRef.current.push(action)

      if (frameRef.current === null) {
        frameRef.current = requestAnimationFrameSafe(flush)
      }
    },
    [flush],
  )

  useEffect(() => cancel, [cancel])

  return useMemo(
    () => ({
      cancel,
      dispatch: batchedDispatch,
    }),
    [batchedDispatch, cancel],
  )
}

function requestAnimationFrameSafe(callback: FrameRequestCallback) {
  if (typeof window.requestAnimationFrame === 'function') {
    return window.requestAnimationFrame(callback)
  }

  return window.setTimeout(() => callback(performance.now()), 16)
}

function cancelAnimationFrameSafe(handle: number) {
  if (typeof window.cancelAnimationFrame === 'function') {
    window.cancelAnimationFrame(handle)
    return
  }

  window.clearTimeout(handle)
}
