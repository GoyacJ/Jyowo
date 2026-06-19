import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/')({
  validateSearch: (search): ConversationSearch => ({
    conversationId:
      typeof search.conversationId === 'string' && search.conversationId.trim().length > 0
        ? search.conversationId
        : undefined,
  }),
})

type ConversationSearch = {
  conversationId?: string
}
