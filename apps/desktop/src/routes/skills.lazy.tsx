import { createLazyFileRoute } from '@tanstack/react-router'

import { SkillSettingsPage } from '@/features/settings/SkillSettings'

export const Route = createLazyFileRoute('/skills')({
  component: SkillSettingsPage,
})
