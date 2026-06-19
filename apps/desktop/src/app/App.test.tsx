import '@testing-library/jest-dom/vitest'

import { QueryClient } from '@tanstack/react-query'
import { act, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { AppProviders } from '@/app/providers'
import { uiStore, useUiStore } from '@/shared/state/ui-store'
import { createMockCommandClient } from '@/shared/tauri/mock-client'
import App from './App'

function mockSystemColorScheme(matches: boolean) {
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
  afterEach(() => {
    window.history.pushState(null, '', '/')
    uiStore.getState().setTheme('light')
    document.documentElement.classList.remove('dark')
    delete document.documentElement.dataset.theme
    vi.restoreAllMocks()
  })

  it('renders the index route through providers and router', async () => {
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: {
          retry: false,
        },
      },
    })

    render(<App commandClient={createMockCommandClient()} queryClient={queryClient} />)

    expect(
      await screen.findByRole('heading', { name: 'Build the desktop foundation' }),
    ).toBeInTheDocument()
    expect(screen.getByRole('navigation', { name: 'Workspace' })).toBeInTheDocument()
    expect(screen.getByRole('complementary', { name: 'Context' })).toBeInTheDocument()
    expect(
      screen.getByPlaceholderText('Ask Jyowo anything about this project...'),
    ).toBeInTheDocument()
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

    render(<App commandClient={createMockCommandClient()} queryClient={queryClient} />)

    expect(await screen.findByRole('heading', { name: 'Memory' })).toBeInTheDocument()
    expect(await screen.findByText('Prefers concise Chinese responses')).toBeInTheDocument()
    expect(screen.getByRole('navigation', { name: 'Workspace' })).toBeInTheDocument()
  })

  it('renders support routes for artifacts, settings, and evals', async () => {
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: {
          retry: false,
        },
      },
    })
    const commandClient = createMockCommandClient()

    window.history.pushState(null, '', '/artifacts')
    const { rerender } = render(<App commandClient={commandClient} queryClient={queryClient} />)

    expect(await screen.findByRole('heading', { name: 'Artifacts' })).toBeInTheDocument()
    expect(screen.getByRole('region', { name: 'Artifact history' })).toBeInTheDocument()

    window.history.pushState(null, '', '/settings')
    rerender(<App commandClient={commandClient} queryClient={queryClient} />)

    expect(await screen.findByRole('heading', { name: 'Settings' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Provider settings' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'MCP servers' })).toBeInTheDocument()

    window.history.pushState(null, '', '/evals')
    rerender(<App commandClient={commandClient} queryClient={queryClient} />)

    expect(await screen.findByRole('heading', { name: 'Eval lab' })).toBeInTheDocument()
    expect(await screen.findByText('Regression smoke')).toBeInTheDocument()
  })

  it('resolves system theme from the operating system preference', () => {
    const systemColorScheme = mockSystemColorScheme(true)

    render(
      <AppProviders commandClient={createMockCommandClient()}>
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
})
