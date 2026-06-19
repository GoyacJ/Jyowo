import { createLazyFileRoute } from '@tanstack/react-router'

import { MemoryBrowser } from '@/features/memory/MemoryBrowser'

export const Route = createLazyFileRoute('/memory')({
  component: MemoryBrowser,
})
