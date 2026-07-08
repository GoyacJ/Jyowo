import { createFileRoute } from '@tanstack/react-router'

const settingsTabs = new Set([
  'general',
  'skills',
  'tools',
  'automations',
  'mcp',
  'plugins',
  'models',
  'about',
])

export const Route = createFileRoute('/settings')({
  validateSearch: (search): SettingsSearch => ({
    tab: typeof search.tab === 'string' && settingsTabs.has(search.tab) ? search.tab : undefined,
  }),
})

type SettingsSearch = {
  tab?: string
}
