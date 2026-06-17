import { createRootRoute, Outlet } from '@tanstack/react-router'

import { RouteErrorMessage } from '@/app/error-boundary'
import { AppShell } from '@/app/shell/AppShell'

export const Route = createRootRoute({
  component: RootComponent,
  errorComponent: ({ error }) => (
    <AppShell>
      <RouteErrorMessage error={error} />
    </AppShell>
  ),
})

function RootComponent() {
  return (
    <AppShell>
      <Outlet />
    </AppShell>
  )
}
