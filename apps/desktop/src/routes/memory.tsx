import { createFileRoute } from '@tanstack/react-router'

const memoryTabs = new Set(['items', 'inbox', 'traces', 'settings'])

export const Route = createFileRoute('/memory')({
  validateSearch: (search): MemorySearch => ({
    tab: typeof search.tab === 'string' && memoryTabs.has(search.tab) ? search.tab : undefined,
  }),
})

type MemorySearch = {
  tab?: string
}
