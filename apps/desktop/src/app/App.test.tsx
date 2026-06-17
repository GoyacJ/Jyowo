import '@testing-library/jest-dom/vitest'

import { QueryClient } from '@tanstack/react-query'
import { act, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { AppProviders } from '@/app/providers'
import { useUiStore } from '@/shared/state/ui-store'
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

    expect(await screen.findByRole('heading', { name: 'Jyowo' })).toBeInTheDocument()
    expect(screen.getAllByText('tauri2-react')[0]).toBeInTheDocument()
    expect(screen.getAllByText('jyowo_harness_sdk')[0]).toBeInTheDocument()
    expect(screen.getByText('available')).toBeInTheDocument()
  })

  it('resolves system theme from the operating system preference', () => {
    const systemColorScheme = mockSystemColorScheme(true)

    render(
      <AppProviders commandClient={createMockCommandClient()}>
        <ThemeSetter theme="system" />
      </AppProviders>,
    )

    expect(document.documentElement).toHaveClass('dark')
    expect(document.documentElement.dataset.theme).toBe('system')

    act(() => {
      systemColorScheme.setMatches(false)
    })

    expect(document.documentElement).not.toHaveClass('dark')
    expect(document.documentElement.dataset.theme).toBe('system')
  })
})
