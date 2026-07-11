import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import type { ReactNode } from 'react'
import { useEffect, useState } from 'react'
import type { DaemonClient } from '@/shared/daemon/client'
import { AppI18nProvider } from '@/shared/i18n/i18n'
import { readUiPreferences, writeUiPreferences } from '@/shared/local-store/ui-preferences-store'
import { useUiStore } from '@/shared/state/ui-store'
import type { CommandClient } from '@/shared/tauri/commands'
import { createDefaultDaemonClient } from '@/shared/tauri/default-client'
import { CommandClientProvider, DaemonClientProvider } from '@/shared/tauri/react'

export interface AppProvidersProps {
  children: ReactNode
  commandClient: CommandClient
  daemonClient?: DaemonClient
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
    const themeColor = getComputedStyle(document.documentElement)
      .getPropertyValue('--background')
      .trim()
    if (themeColor) {
      document.querySelector('meta[name="theme-color"]')?.setAttribute('content', themeColor)
    }
  }, [systemPrefersDark, theme])

  return children
}

function UiPreferencesProvider({ children }: { children: ReactNode }) {
  const theme = useUiStore((state) => state.theme)
  const locale = useUiStore((state) => state.locale)
  const sidebarCollapsed = useUiStore((state) => state.sidebarCollapsed)
  const taskWorkbenchMode = useUiStore((state) => state.taskWorkbenchMode)
  const setTheme = useUiStore((state) => state.setTheme)
  const setLocale = useUiStore((state) => state.setLocale)
  const setSidebarCollapsed = useUiStore((state) => state.setSidebarCollapsed)
  const setTaskWorkbenchMode = useUiStore((state) => state.setTaskWorkbenchMode)
  const [hydrated, setHydrated] = useState(false)

  useEffect(() => {
    let cancelled = false

    void readUiPreferences()
      .then((preferences) => {
        if (cancelled) {
          return
        }

        setTheme(preferences.theme)
        setLocale(preferences.locale)
        setSidebarCollapsed(preferences.sidebarCollapsed)
        setTaskWorkbenchMode(preferences.taskWorkbenchMode)
        setHydrated(true)
      })
      .catch(() => {
        if (cancelled) {
          return
        }

        // Local UI preferences are non-security settings, so store failures should not block app rendering.
        setHydrated(true)
      })

    return () => {
      cancelled = true
    }
  }, [setLocale, setSidebarCollapsed, setTaskWorkbenchMode, setTheme])

  useEffect(() => {
    if (!hydrated) {
      return
    }

    void writeUiPreferences({ locale, sidebarCollapsed, taskWorkbenchMode, theme }).catch(() => {
      // Local UI preferences are non-security settings; the app can keep running without persistence.
    })
  }, [hydrated, locale, sidebarCollapsed, taskWorkbenchMode, theme])

  return children
}

export function AppProviders({
  children,
  commandClient,
  daemonClient,
  queryClient,
}: AppProvidersProps) {
  const [defaultQueryClient] = useState(createQueryClient)
  const [defaultDaemonClient] = useState(createDefaultDaemonClient)

  return (
    <DaemonClientProvider client={daemonClient ?? defaultDaemonClient}>
      <CommandClientProvider client={commandClient}>
        <QueryClientProvider client={queryClient ?? defaultQueryClient}>
          <UiPreferencesProvider>
            <AppI18nProvider>
              <ThemeProvider>{children}</ThemeProvider>
            </AppI18nProvider>
          </UiPreferencesProvider>
        </QueryClientProvider>
      </CommandClientProvider>
    </DaemonClientProvider>
  )
}
