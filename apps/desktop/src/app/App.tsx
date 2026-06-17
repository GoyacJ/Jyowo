import type { QueryClient } from '@tanstack/react-query'
import { RouterProvider } from '@tanstack/react-router'
import { useMemo, useState } from 'react'
import type { CommandClient } from '@/shared/tauri/commands'
import { createDefaultCommandClient } from '@/shared/tauri/default-client'
import { AppProviders } from './providers'
import { createAppRouter } from './router'

export interface AppProps {
  commandClient?: CommandClient
  queryClient?: QueryClient
}

export default function App({ commandClient, queryClient }: AppProps) {
  const [router] = useState(() => createAppRouter())
  const resolvedCommandClient = useMemo(
    () => commandClient ?? createDefaultCommandClient(),
    [commandClient],
  )

  return (
    <AppProviders commandClient={resolvedCommandClient} queryClient={queryClient}>
      <RouterProvider router={router} />
    </AppProviders>
  )
}
