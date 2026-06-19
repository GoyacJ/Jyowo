import { createLazyFileRoute } from '@tanstack/react-router'

import { ConversationWorkspace } from '@/features/conversation/ConversationWorkspace'

export const Route = createLazyFileRoute('/')({
  component: ConversationRoute,
})

function ConversationRoute() {
  const { conversationId } = Route.useSearch()

  return <ConversationWorkspace conversationId={conversationId} />
}
