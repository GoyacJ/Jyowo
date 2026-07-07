import type {
  CommandClient,
  ConversationCursor,
  ConversationEventBatchPayload,
  SubscribeConversationEventsResponse,
} from '@/shared/tauri/commands'
import type { ConversationTimelineAction } from './conversation-timeline-actions'

export type ConversationTimelineSource = {
  subscribe: (
    conversationId: string,
    afterCursor: ConversationCursor | null,
    dispatch: (action: ConversationTimelineAction) => void,
  ) => Promise<() => Promise<void>>
}

export function createConversationTimelineSource(
  commandClient: CommandClient,
): ConversationTimelineSource {
  return {
    async subscribe(conversationId, afterCursor, dispatch) {
      let active = true
      let subscription: SubscribeConversationEventsResponse | null = null
      let resubscribeTask = Promise.resolve()
      const subscribeOnce = async (
        cursor: ConversationCursor | null,
        { emitIdleRefresh }: { emitIdleRefresh: boolean },
      ) => {
        const nextSubscription = await commandClient.subscribeConversationEvents({
          conversationId,
          ...(cursor ? { afterCursor: cursor } : {}),
        })
        if (!active) {
          await commandClient
            .unsubscribeConversationEvents(nextSubscription.subscriptionId)
            .catch(() => undefined)
          return
        }
        subscription = nextSubscription
        const immediate = nextSubscription.replayEvents.length > 0 || nextSubscription.gap
        if (immediate || emitIdleRefresh) {
          dispatch({
            type: 'worktreeRefreshRequested',
            immediate,
          })
        }
      }
      const resubscribe = (cursor: ConversationCursor | null) => {
        resubscribeTask = resubscribeTask.then(async () => {
          if (!active) {
            return
          }
          const previous = subscription
          await subscribeOnce(cursor, { emitIdleRefresh: false }).catch(() => {
            dispatch({ type: 'worktreeRefreshRequested', immediate: true })
          })
          if (previous && previous.subscriptionId !== subscription?.subscriptionId) {
            await commandClient
              .unsubscribeConversationEvents(previous.subscriptionId)
              .catch(() => undefined)
          }
        })
        void resubscribeTask
      }
      const unlisten = await commandClient.listenConversationEventBatches((batch) => {
        if (!active || !subscription || isStaleBatch(batch, subscription)) {
          return
        }
        dispatchBatch(batch, dispatch)
        if (batch.gap) {
          resubscribe(batch.cursor ?? null)
        }
      })

      try {
        await subscribeOnce(afterCursor, { emitIdleRefresh: true })
      } catch {
        dispatch({ type: 'worktreeRefreshRequested', immediate: true })
        await subscribeOnce(null, { emitIdleRefresh: false }).catch(() => undefined)
      }

      return async () => {
        active = false
        unlisten()
        await resubscribeTask.catch(() => undefined)
        if (subscription) {
          await commandClient
            .unsubscribeConversationEvents(subscription.subscriptionId)
            .catch(() => undefined)
        }
      }
    },
  }
}

function isStaleBatch(
  batch: ConversationEventBatchPayload,
  subscription: SubscribeConversationEventsResponse,
) {
  return (
    batch.subscriptionId !== subscription.subscriptionId ||
    batch.conversationId !== subscription.conversationId
  )
}

function dispatchBatch(
  batch: ConversationEventBatchPayload,
  dispatch: (action: ConversationTimelineAction) => void,
) {
  dispatch({
    type: 'worktreeRefreshRequested',
    immediate: batch.gap || batch.events.some(isTerminalProjectionSignal),
  })
}

function isTerminalProjectionSignal(event: ConversationEventBatchPayload['events'][number]) {
  return (
    event.type === 'run.ended' ||
    event.type === 'engine.failed' ||
    event.type === 'tool.completed' ||
    event.type === 'tool.failed' ||
    event.type === 'tool.denied' ||
    event.type === 'permission.resolved'
  )
}
