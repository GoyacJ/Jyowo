import { createFileRoute } from '@tanstack/react-router'

import { ConversationWorkspace } from '@/features/conversation/ConversationWorkspace'

export const Route = createFileRoute('/')({
  component: ConversationWorkspace,
})
