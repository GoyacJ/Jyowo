import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { getCurrentWindow } from '@tauri-apps/api/window'
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

const PREFERENCE_HYDRATION_TIMEOUT_MS = 1_500
const WINDOW_SHOW_RETRY_DELAY_MS = 250
const WINDOW_SHOW_ATTEMPT_TIMEOUT_MS = 1_000
const WINDOW_SHOW_MAX_ATTEMPTS = 3

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

function applyTheme(theme: 'dark' | 'light' | 'system', systemPrefersDark: boolean) {
  const resolvedTheme = theme === 'system' ? (systemPrefersDark ? 'dark' : 'light') : theme

  document.documentElement.classList.toggle('dark', resolvedTheme === 'dark')
  document.documentElement.dataset.theme = theme
  const themeColor = getComputedStyle(document.documentElement)
    .getPropertyValue('--background')
    .trim()
  if (themeColor) {
    document.querySelector('meta[name="theme-color"]')?.setAttribute('content', themeColor)
  }
}

function showTauriWindowAfterTheme() {
  if (!('__TAURI_INTERNALS__' in window)) return () => undefined

  let attempt = 0
  let cancelled = false
  let attemptTimeout: number | undefined
  let retryTimeout: number | undefined

  const clearAttemptTimeout = () => {
    if (attemptTimeout === undefined) return
    window.clearTimeout(attemptTimeout)
    attemptTimeout = undefined
  }

  const scheduleRetry = () => {
    if (cancelled || attempt >= WINDOW_SHOW_MAX_ATTEMPTS) return
    retryTimeout = window.setTimeout(runAttempt, WINDOW_SHOW_RETRY_DELAY_MS)
  }

  const runAttempt = () => {
    if (cancelled) return

    attempt += 1
    let settled = false
    attemptTimeout = window.setTimeout(() => {
      if (cancelled || settled) return
      settled = true
      attemptTimeout = undefined
      scheduleRetry()
    }, WINDOW_SHOW_ATTEMPT_TIMEOUT_MS)

    void Promise.resolve()
      .then(() => getCurrentWindow().show())
      .then(
        () => {
          if (cancelled || settled) return
          settled = true
          clearAttemptTimeout()
        },
        () => {
          if (cancelled || settled) return
          settled = true
          clearAttemptTimeout()
          scheduleRetry()
        },
      )
  }

  runAttempt()

  return () => {
    cancelled = true
    clearAttemptTimeout()
    if (retryTimeout !== undefined) {
      window.clearTimeout(retryTimeout)
    }
  }
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
    applyTheme(theme, systemPrefersDark)
  }, [systemPrefersDark, theme])

  return children
}

function UiPreferencesProvider({ children }: { children: ReactNode }) {
  const theme = useUiStore((state) => state.theme)
  const locale = useUiStore((state) => state.locale)
  const sidebarCollapsed = useUiStore((state) => state.sidebarCollapsed)
  const sidebarSections = useUiStore((state) => state.sidebarSections)
  const expandedProjects = useUiStore((state) => state.expandedProjects)
  const taskWorkbenchWidth = useUiStore((state) => state.taskWorkbenchWidth)
  const setTheme = useUiStore((state) => state.setTheme)
  const setLocale = useUiStore((state) => state.setLocale)
  const setSidebarCollapsed = useUiStore((state) => state.setSidebarCollapsed)
  const setSidebarSectionExpanded = useUiStore((state) => state.setSidebarSectionExpanded)
  const setProjectExpanded = useUiStore((state) => state.setProjectExpanded)
  const setTaskWorkbenchWidth = useUiStore((state) => state.setTaskWorkbenchWidth)
  const [hydrated, setHydrated] = useState(false)

  useEffect(() => {
    let cancelled = false
    let revealStarted = false
    let cancelReveal: (() => void) | undefined
    const revealWindow = () => {
      if (cancelled || revealStarted) return
      revealStarted = true
      cancelReveal = showTauriWindowAfterTheme()
    }
    const revealTimeout = window.setTimeout(revealWindow, PREFERENCE_HYDRATION_TIMEOUT_MS)

    void readUiPreferences()
      .then((preferences) => {
        if (cancelled) {
          return
        }

        setTheme(preferences.theme)
        setLocale(preferences.locale)
        setSidebarCollapsed(preferences.sidebarCollapsed)
        setSidebarSectionExpanded('pinned', preferences.sidebarSections.pinned)
        setSidebarSectionExpanded('projects', preferences.sidebarSections.projects)
        setSidebarSectionExpanded('conversations', preferences.sidebarSections.conversations)
        for (const [path, expanded] of Object.entries(preferences.expandedProjects)) {
          setProjectExpanded(path, expanded)
        }
        setTaskWorkbenchWidth(preferences.taskWorkbenchWidth)
        applyTheme(preferences.theme, getSystemPrefersDark())
        setHydrated(true)
        revealWindow()
      })
      .catch(() => {
        if (cancelled) {
          return
        }

        // Local UI preferences are non-security settings, so store failures should not block app rendering.
        setHydrated(true)
        revealWindow()
      })

    return () => {
      cancelled = true
      window.clearTimeout(revealTimeout)
      cancelReveal?.()
    }
  }, [
    setLocale,
    setProjectExpanded,
    setSidebarCollapsed,
    setSidebarSectionExpanded,
    setTaskWorkbenchWidth,
    setTheme,
  ])

  useEffect(() => {
    if (!hydrated) {
      return
    }

    void writeUiPreferences({
      expandedProjects,
      locale,
      sidebarCollapsed,
      sidebarSections,
      taskWorkbenchWidth,
      theme,
    }).catch(() => {
      // Local UI preferences are non-security settings; the app can keep running without persistence.
    })
  }, [
    expandedProjects,
    hydrated,
    locale,
    sidebarCollapsed,
    sidebarSections,
    taskWorkbenchWidth,
    theme,
  ])

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
