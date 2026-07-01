import { createLazyFileRoute } from '@tanstack/react-router'

import { BackgroundAgentsPanel } from '@/features/background-agents/BackgroundAgentsPanel'

export const Route = createLazyFileRoute('/background-agents')({
  component: BackgroundAgentsRoute,
})

function BackgroundAgentsRoute() {
  const { backgroundAgentId } = Route.useSearch()

  return <BackgroundAgentsPanel selectedBackgroundAgentId={backgroundAgentId} />
}
