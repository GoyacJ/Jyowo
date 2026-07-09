import { createFileRoute } from '@tanstack/react-router'

const skillsTabs = new Set(['skills', 'tools', 'mcp', 'plugins'])

export const Route = createFileRoute('/skills')({
  validateSearch: (search): SkillsSearch => ({
    tab: typeof search.tab === 'string' && skillsTabs.has(search.tab) ? search.tab : undefined,
  }),
})

type SkillsSearch = {
  tab?: string
}
