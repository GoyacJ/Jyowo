import { createFileRoute } from '@tanstack/react-router'

const memoryTabs = new Set(['items', 'inbox', 'traces', 'settings'])

export const Route = createFileRoute('/memory')({
  validateSearch: (search): MemorySearch => ({
    tab: typeof search.tab === 'string' && memoryTabs.has(search.tab) ? search.tab : undefined,
    workspaceRoot: typeof search.workspaceRoot === 'string' ? search.workspaceRoot : undefined,
  }),
})

type MemorySearch = {
  tab?: string
  workspaceRoot?: string
}
