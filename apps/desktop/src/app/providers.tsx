import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import type { ReactNode } from 'react'
import { useEffect, useState } from 'react'

import { useUiStore } from '@/shared/state/ui-store'
import type { CommandClient } from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'

export interface AppProvidersProps {
  children: ReactNode
  commandClient: CommandClient
  queryClient?: QueryClient
}

function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
        staleTime: 30_000,
      },
    },
  })
}

function ThemeProvider({ children }: { children: ReactNode }) {
  const theme = useUiStore((state) => state.theme)

  useEffect(() => {
    document.documentElement.classList.toggle('dark', theme === 'dark')
    document.documentElement.dataset.theme = theme
  }, [theme])

  return children
}

export function AppProviders({ children, commandClient, queryClient }: AppProvidersProps) {
  const [defaultQueryClient] = useState(createQueryClient)

  return (
    <CommandClientProvider client={commandClient}>
      <QueryClientProvider client={queryClient ?? defaultQueryClient}>
        <ThemeProvider>{children}</ThemeProvider>
      </QueryClientProvider>
    </CommandClientProvider>
  )
}
