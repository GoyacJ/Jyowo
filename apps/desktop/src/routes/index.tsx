import { createFileRoute } from '@tanstack/react-router'

import { SystemStatusPage } from '@/features/system-status/SystemStatusPage'

export const Route = createFileRoute('/')({
  component: SystemStatusPage,
})
