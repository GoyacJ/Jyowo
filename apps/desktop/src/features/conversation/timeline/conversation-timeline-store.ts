import type { ConversationTimelineAction } from './conversation-timeline-actions'
import {
  type ConversationTimelineState,
  conversationTimelineReducer,
  createConversationTimelineState,
} from './conversation-timeline-reducer'

export type ConversationTimelineRoot = {
  byConversationId: Record<string, ConversationTimelineState>
}

export function createConversationTimelineRoot(): ConversationTimelineRoot {
  return { byConversationId: {} }
}

export function getConversationTimelineState(
  root: ConversationTimelineRoot,
  conversationId: string,
): ConversationTimelineState {
  return root.byConversationId[conversationId] ?? createConversationTimelineState(conversationId)
}

function conversationTimelineRootReducer(
  root: ConversationTimelineRoot,
  conversationId: string,
  action: ConversationTimelineAction,
): ConversationTimelineRoot {
  const current = getConversationTimelineState(root, conversationId)
  const next = conversationTimelineReducer(current, action)

  if (next === current) {
    return root
  }

  return {
    byConversationId: {
      ...root.byConversationId,
      [conversationId]: next,
    },
  }
}

export type ConversationTimelineRootAction = {
  conversationId: string
  action: ConversationTimelineAction
}

export function conversationTimelineRootReducerFromAction(
  root: ConversationTimelineRoot,
  scoped: ConversationTimelineRootAction,
): ConversationTimelineRoot {
  return conversationTimelineRootReducer(root, scoped.conversationId, scoped.action)
}
