import type { QueryClient } from '@tanstack/react-query'
import { RouterProvider } from '@tanstack/react-router'
import { useMemo, useState } from 'react'
import type { DaemonClient } from '@/shared/daemon/client'
import type { CommandClient } from '@/shared/tauri/commands'
import { createDefaultCommandClient } from '@/shared/tauri/default-client'
import { AppProviders } from './providers'
import { createAppRouter } from './router'

export interface AppProps {
  commandClient?: CommandClient
  daemonClient?: DaemonClient
  queryClient?: QueryClient
}

export default function App({ commandClient, daemonClient, queryClient }: AppProps) {
  const [router] = useState(() => createAppRouter())
  const resolvedCommandClient = useMemo(
    () => commandClient ?? createDefaultCommandClient(),
    [commandClient],
  )

  return (
    <AppProviders
      commandClient={resolvedCommandClient}
      daemonClient={daemonClient}
      queryClient={queryClient}
    >
      <RouterProvider router={router} />
    </AppProviders>
  )
}
