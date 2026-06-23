import { createLazyFileRoute, useNavigate } from '@tanstack/react-router'

import { ConversationWorkspace } from '@/features/conversation/ConversationWorkspace'
import { WelcomeWorkspace } from '@/features/conversation/WelcomeWorkspace'

export const Route = createLazyFileRoute('/')({
  component: ConversationRoute,
})

function ConversationRoute() {
  const { conversationId } = Route.useSearch()
  const navigate = useNavigate()

  if (!conversationId) {
    return (
      <WelcomeWorkspace
        onConversationCreated={(nextConversationId) => {
          void navigate({ search: { conversationId: nextConversationId }, to: '/' })
        }}
      />
    )
  }

  return <ConversationWorkspace conversationId={conversationId} />
}
