import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { appI18n } from '@/shared/i18n/i18n'
import type { CommandClient } from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'

import { WelcomeWorkspace } from './WelcomeWorkspace'

const navigate = vi.hoisted(() => vi.fn())

vi.mock('@tanstack/react-router', async () => ({
  useNavigate: () => navigate,
}))

function renderWelcomeWorkspace(commandClient: CommandClient, onConversationCreated = vi.fn()) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  })

  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <CommandClientProvider client={commandClient}>
        <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
      </CommandClientProvider>
    )
  }

  return {
    onConversationCreated,
    ...render(<WelcomeWorkspace onConversationCreated={onConversationCreated} />, {
      wrapper: Wrapper,
    }),
  }
}

describe('WelcomeWorkspace', () => {
  beforeEach(async () => {
    navigate.mockClear()
    await appI18n.changeLanguage('en-US')
  })

  it('creates conversations when no project is active', async () => {
    const createConversation = vi.fn(async () => ({
      conversation: {
        id: 'conversation-no-workspace-created',
        isEmpty: true,
        lastMessagePreview: 'Start from the composer when ready.',
        title: 'New conversation',
        updatedAt: '2026-06-17T00:00:00.000Z',
      },
    }))
    const onConversationCreated = vi.fn()

    renderWelcomeWorkspace(
      {
        ...createTestCommandClient({ projects: { activePath: null, projects: [] } }),
        createConversation,
      },
      onConversationCreated,
    )

    fireEvent.click(await screen.findByRole('button', { name: 'New conversation' }))

    await waitFor(() => {
      expect(createConversation).toHaveBeenCalledTimes(1)
    })
    expect(onConversationCreated).toHaveBeenCalledWith('conversation-no-workspace-created')
  })
})
