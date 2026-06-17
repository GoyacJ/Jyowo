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

function getSystemPrefersDark() {
  return window.matchMedia?.('(prefers-color-scheme: dark)').matches ?? false
}

function ThemeProvider({ children }: { children: ReactNode }) {
  const theme = useUiStore((state) => state.theme)
  const [systemPrefersDark, setSystemPrefersDark] = useState(getSystemPrefersDark)

  useEffect(() => {
    if (theme !== 'system') {
      return
    }

    const mediaQuery = window.matchMedia?.('(prefers-color-scheme: dark)')

    if (!mediaQuery) {
      setSystemPrefersDark(false)
      return
    }

    setSystemPrefersDark(mediaQuery.matches)

    function handleChange(event: MediaQueryListEvent) {
      setSystemPrefersDark(event.matches)
    }

    mediaQuery.addEventListener('change', handleChange)

    return () => {
      mediaQuery.removeEventListener('change', handleChange)
    }
  }, [theme])

  useEffect(() => {
    const resolvedTheme = theme === 'system' ? (systemPrefersDark ? 'dark' : 'light') : theme

    document.documentElement.classList.toggle('dark', resolvedTheme === 'dark')
    document.documentElement.dataset.theme = theme
  }, [systemPrefersDark, theme])

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
