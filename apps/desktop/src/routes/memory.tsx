import { createFileRoute } from '@tanstack/react-router'

import { MemoryBrowser } from '@/features/memory/MemoryBrowser'

export const Route = createFileRoute('/memory')({
  component: MemoryBrowser,
})
