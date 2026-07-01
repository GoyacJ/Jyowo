import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/background-agents')({
  validateSearch: (search): BackgroundAgentsSearch => ({
    backgroundAgentId:
      typeof search.backgroundAgentId === 'string' && search.backgroundAgentId.trim().length > 0
        ? search.backgroundAgentId
        : undefined,
  }),
})

type BackgroundAgentsSearch = {
  backgroundAgentId?: string
}
