import '@testing-library/jest-dom/vitest'

import { QueryClient } from '@tanstack/react-query'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { AppProviders } from '@/app/providers'
import type { DaemonClient } from '@/shared/daemon/client'
import { uiStore, useUiStore } from '@/shared/state/ui-store'
import { createTestCommandClient, testJyowoProject } from '@/testing/command-client'
import App from './App'

const emptyProviderSettingsList = {
  defaultConfigId: null,
  selectionScope: 'global' as const,
  configs: [],
}

const daemonClient = {
  connect: vi.fn().mockResolvedValue(undefined),
  listTasks: vi.fn().mockResolvedValue({ tasks: [], type: 'task_list' }),
  loadTask: vi.fn(),
  readBlob: vi.fn(),
  request: vi.fn(),
  subscribe: vi.fn().mockResolvedValue(async () => undefined),
} as unknown as DaemonClient

const daemonTaskId = '01J00000000000000000000071'

const uiPreferencesStoreFixture = vi.hoisted(() => ({
  readUiPreferences: vi.fn<
    () => Promise<{
      theme: 'light' | 'dark' | 'system'
      locale: 'zh-CN' | 'en-US'
      sidebarCollapsed: boolean
      taskWorkbenchMode: 'closed' | 'inspector' | 'collaboration'
      chatComposerHeight: number
      contextPanelWidth: number
    }>
  >(async () => ({
    theme: 'light',
    locale: 'en-US',
    sidebarCollapsed: false,
    taskWorkbenchMode: 'closed',
    chatComposerHeight: 160,
    contextPanelWidth: 320,
  })),
  writeUiPreferences: vi.fn(async () => undefined),
}))

vi.mock('@/shared/local-store/ui-preferences-store', () => uiPreferencesStoreFixture)

function setSystemColorSchemeFixture(matches: boolean) {
  const listeners = new Set<EventListenerOrEventListenerObject>()
  const mediaQueryList = {
    matches,
    media: '(prefers-color-scheme: dark)',
    onchange: null,
    addEventListener: vi.fn((_event: string, listener: EventListenerOrEventListenerObject) => {
      listeners.add(listener)
    }),
    removeEventListener: vi.fn((_event: string, listener: EventListenerOrEventListenerObject) => {
      listeners.delete(listener)
    }),
    addListener: vi.fn(),
    removeListener: vi.fn(),
    dispatchEvent: vi.fn(),
  } satisfies MediaQueryList

  Object.defineProperty(window, 'matchMedia', {
    configurable: true,
    value: vi.fn().mockReturnValue(mediaQueryList),
  })

  return {
    setMatches(nextMatches: boolean) {
      mediaQueryList.matches = nextMatches
      const event = { matches: nextMatches } as MediaQueryListEvent
      for (const listener of listeners) {
        if (typeof listener === 'function') {
          listener.call(mediaQueryList, event)
        } else {
          listener.handleEvent(event)
        }
      }
    },
  }
}

function ThemeSetter({ theme }: { theme: 'light' | 'dark' | 'system' }) {
  const setTheme = useUiStore((state) => state.setTheme)

  return (
    <button onClick={() => setTheme(theme)} type="button">
      Set theme
    </button>
  )
}

describe('App', () => {
  beforeEach(() => {
    uiPreferencesStoreFixture.readUiPreferences.mockReset()
    uiPreferencesStoreFixture.readUiPreferences.mockResolvedValue({
      theme: 'light',
      locale: 'en-US',
      sidebarCollapsed: false,
      taskWorkbenchMode: 'closed',
      chatComposerHeight: 160,
      contextPanelWidth: 320,
    })
    uiPreferencesStoreFixture.writeUiPreferences.mockReset()
    uiPreferencesStoreFixture.writeUiPreferences.mockResolvedValue(undefined)
  })

  afterEach(() => {
    window.history.pushState(null, '', '/')
    uiStore.getState().setTheme('light')
    uiStore.getState().setLocale('en-US')
    uiStore.getState().setSidebarCollapsed(false)
    uiStore.getState().setTaskWorkbenchMode('closed')
    document.documentElement.classList.remove('dark')
    delete document.documentElement.dataset.theme
    vi.restoreAllMocks()
  })

  it('renders the index route through providers and router', async () => {
    window.history.pushState(null, '', `/?taskId=${daemonTaskId}`)
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: {
          retry: false,
        },
      },
    })

    const taskDaemonClient = {
      ...daemonClient,
      listTasks: vi.fn().mockResolvedValue({
        tasks: [
          {
            archived: false,
            lastGlobalOffset: 0,
            queue: [],
            state: 'idle',
            streamVersion: 0,
            taskId: daemonTaskId,
            title: 'Build the desktop foundation',
          },
        ],
        type: 'task_list',
      }),
      loadTask: vi.fn().mockResolvedValue({
        projection: {
          archived: false,
          lastGlobalOffset: 0,
          queue: [],
          state: 'idle',
          streamVersion: 0,
          taskId: daemonTaskId,
          title: 'Build the desktop foundation',
        },
        snapshotOffset: 0,
        timeline: [],
      }),
    } as unknown as DaemonClient

    render(
      <App
        commandClient={createTestCommandClient({ projects: testJyowoProject })}
        daemonClient={taskDaemonClient}
        queryClient={queryClient}
      />,
    )

    expect(
      await screen.findByRole('heading', { name: 'Build the desktop foundation' }),
    ).toBeInTheDocument()
    expect(screen.getByRole('complementary', { name: 'Workspace' })).toBeInTheDocument()
    expect(screen.queryByRole('complementary', { name: 'Context' })).not.toBeInTheDocument()
    expect(screen.getByRole('region', { name: 'Status' })).toBeInTheDocument()
    expect(
      screen.getByPlaceholderText('Ask Jyowo anything about this project…'),
    ).toBeInTheDocument()
  })

  it('renders the welcome page when no conversation is selected', async () => {
    window.history.pushState(null, '', '/')
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: {
          retry: false,
        },
      },
    })

    render(
      <App
        commandClient={createTestCommandClient()}
        daemonClient={daemonClient}
        queryClient={queryClient}
      />,
    )

    expect(await screen.findByRole('heading', { name: 'Choose a task' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'New task' })).toBeInTheDocument()
  })

  it('renders the memory browser support route', async () => {
    window.history.pushState(null, '', '/memory')
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: {
          retry: false,
        },
      },
    })

    render(
      <App
        commandClient={createTestCommandClient()}
        daemonClient={daemonClient}
        queryClient={queryClient}
      />,
    )

    expect(await screen.findByRole('heading', { name: 'Memory' })).toBeInTheDocument()
    expect(await screen.findByText('Prefers concise Chinese responses')).toBeInTheDocument()
    expect(screen.getByRole('complementary', { name: 'Workspace' })).toBeInTheDocument()
  })

  it('renders support routes for skills, settings, and evals', async () => {
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: {
          retry: false,
        },
      },
    })
    const commandClient = createTestCommandClient({
      providerSettingsList: emptyProviderSettingsList,
      runtimeTools: {
        generation: 3,
        tools: [
          {
            name: 'FileRead',
            displayName: 'Read file',
            description: 'Read a file from the workspace.',
            category: 'builtin',
            group: 'fileSystem',
            groupLabel: 'File system',
            originKind: 'builtin',
            originId: null,
            access: 'readOnly',
            executionChannel: 'directAuthorizedRust',
            requiredCapabilities: [],
            deferPolicy: 'alwaysLoad',
            longRunning: false,
            serviceBinding: null,
          },
          {
            name: 'MiniMaxTextToImage',
            displayName: 'MiniMax text to image',
            description: 'Generate images with MiniMax.',
            category: 'builtin',
            group: 'network',
            groupLabel: 'Network',
            originKind: 'builtin',
            originId: null,
            access: 'mutating',
            executionChannel: 'httpBroker',
            requiredCapabilities: ['provider_credential_resolver'],
            deferPolicy: 'alwaysLoad',
            longRunning: true,
            serviceBinding: {
              providerId: 'minimax',
              operationId: 'minimax.image_generation',
              routeKind: 'imageGeneration',
            },
          },
        ],
      },
    })

    window.history.pushState(null, '', '/skills')
    const { rerender } = render(
      <App commandClient={commandClient} daemonClient={daemonClient} queryClient={queryClient} />,
    )

    expect(await screen.findByRole('region', { name: 'Skills' })).toBeInTheDocument()
    expect(screen.queryByRole('heading', { level: 1, name: 'Skills' })).not.toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'Skills' })).toHaveAttribute('aria-selected', 'true')
    expect(screen.getByRole('tab', { name: 'Tools' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'MCP' })).toBeInTheDocument()
    expect(
      await screen.findByRole('button', { name: /Creates release notes from recent changes/ }),
    ).toBeInTheDocument()

    fireEvent.mouseDown(screen.getByRole('tab', { name: 'Tools' }))

    expect(await screen.findByRole('heading', { name: 'Runtime tools' })).toBeInTheDocument()
    expect(await screen.findByText('FileRead')).toBeInTheDocument()
    expect(await screen.findByText('MiniMaxTextToImage')).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: 'Model configuration' })).not.toBeInTheDocument()

    fireEvent.mouseDown(screen.getByRole('tab', { name: 'MCP' }))

    expect(await screen.findByRole('heading', { name: 'MCP servers' })).toBeInTheDocument()

    window.history.pushState(null, '', '/settings')
    rerender(
      <App commandClient={commandClient} daemonClient={daemonClient} queryClient={queryClient} />,
    )

    expect(await screen.findByRole('region', { name: 'Settings' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'General' })).toHaveAttribute('aria-selected', 'true')
    expect(screen.getByRole('tab', { name: 'Skills' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'Tools' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'MCP' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'Models' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Language' })).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: 'Model configuration' })).not.toBeInTheDocument()

    fireEvent.mouseDown(screen.getByRole('tab', { name: 'Models' }))

    expect(await screen.findByRole('heading', { name: 'Models' })).toBeInTheDocument()
    expect(await screen.findByRole('heading', { name: 'No configured models' })).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: 'Model configuration' })).not.toBeInTheDocument()

    window.history.pushState(null, '', '/evals')
    rerender(
      <App commandClient={commandClient} daemonClient={daemonClient} queryClient={queryClient} />,
    )

    expect(await screen.findByRole('heading', { name: 'Eval lab' })).toBeInTheDocument()
    expect(await screen.findByText('Regression smoke')).toBeInTheDocument()
  })

  it('resolves system theme from the operating system preference', () => {
    const systemColorScheme = setSystemColorSchemeFixture(true)

    render(
      <AppProviders commandClient={createTestCommandClient()}>
        <ThemeSetter theme="system" />
      </AppProviders>,
    )

    act(() => {
      screen.getByRole('button', { name: 'Set theme' }).click()
    })

    expect(document.documentElement).toHaveClass('dark')
    expect(document.documentElement.dataset.theme).toBe('system')

    act(() => {
      systemColorScheme.setMatches(false)
    })

    expect(document.documentElement).not.toHaveClass('dark')
    expect(document.documentElement.dataset.theme).toBe('system')
  })

  it('hydrates and persists local UI preferences', async () => {
    uiPreferencesStoreFixture.readUiPreferences.mockResolvedValueOnce({
      theme: 'dark',
      locale: 'en-US',
      sidebarCollapsed: true,
      taskWorkbenchMode: 'collaboration',
      chatComposerHeight: 160,
      contextPanelWidth: 320,
    })

    render(
      <AppProviders commandClient={createTestCommandClient()}>
        <ThemeSetter theme="system" />
      </AppProviders>,
    )

    await waitFor(() => {
      expect(uiStore.getState().theme).toBe('dark')
      expect(uiStore.getState().sidebarCollapsed).toBe(true)
      expect(uiStore.getState().taskWorkbenchMode).toBe('collaboration')
      expect(document.documentElement).toHaveClass('dark')
    })

    act(() => {
      screen.getByRole('button', { name: 'Set theme' }).click()
    })

    expect(uiPreferencesStoreFixture.writeUiPreferences).toHaveBeenCalledWith({
      locale: 'en-US',
      theme: 'system',
      sidebarCollapsed: true,
      taskWorkbenchMode: 'collaboration',
    })
  })
})
