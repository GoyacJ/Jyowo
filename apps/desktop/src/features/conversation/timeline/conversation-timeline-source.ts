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
      const unlisten = await commandClient.listenConversationEventBatches((batch) => {
        if (!active || !subscription || isStaleBatch(batch, subscription)) {
          return
        }
        dispatchBatch(batch, dispatch)
      })

      try {
        subscription = await commandClient.subscribeConversationEvents({
          conversationId,
          ...(afterCursor ? { afterCursor } : {}),
        })
        dispatch({
          type: 'applyEvents',
          events: subscription.replayEvents,
          cursor: subscription.cursor ?? null,
        })
        if (subscription.gap) {
          dispatch({ type: 'markGap', afterCursor: subscription.cursor ?? undefined })
        }
      } catch {
        dispatch({ type: 'markGap', afterCursor: afterCursor ?? undefined })
      }

      return async () => {
        active = false
        unlisten()
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
  dispatch({ type: 'applyEvents', events: batch.events, cursor: batch.cursor ?? null })
  if (batch.gap) {
    dispatch({ type: 'markGap', afterCursor: batch.cursor ?? undefined })
  }
}
