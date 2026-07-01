import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import { createElement, type ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'

import type { CommandClient } from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createRejectedTestCommandClient } from '@/testing/command-client'

import { useAgentProfiles } from './use-agent-profiles'

const workspacePath = '/tmp/jyowo-project'

vi.mock('@/features/workspace/use-active-project-path', () => ({
  useActiveProjectPath: () => ({
    data: workspacePath,
    error: null,
    isLoading: false,
  }),
}))

function renderUseAgentProfiles(commandClient: CommandClient) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  })

  function Probe() {
    const profiles = useAgentProfiles()

    if (profiles.isLoading) {
      return createElement('div', null, 'Loading profiles')
    }

    if (profiles.error) {
      return createElement('div', null, profiles.error.message)
    }

    if (profiles.isEmpty) {
      return createElement('div', null, 'No profiles')
    }

    return createElement('div', null, profiles.profiles.map((profile) => profile.id).join(','))
  }

  function Wrapper({ children }: { children: ReactNode }) {
    return createElement(CommandClientProvider, {
      children: createElement(QueryClientProvider, { children, client: queryClient }),
      client: commandClient,
    })
  }

  return render(createElement(Probe), { wrapper: Wrapper })
}

describe('useAgentProfiles', () => {
  it('shows loading state while profiles are pending', () => {
    renderUseAgentProfiles({
      listAgentProfiles: () => new Promise(() => {}),
    } as unknown as CommandClient)

    expect(screen.getByText('Loading profiles')).toBeInTheDocument()
  })

  it('shows empty state when backend returns no profiles', async () => {
    renderUseAgentProfiles({
      listAgentProfiles: vi.fn().mockResolvedValue({ profiles: [] }),
    } as unknown as CommandClient)

    expect(await screen.findByText('No profiles')).toBeInTheDocument()
  })

  it('surfaces command errors', async () => {
    renderUseAgentProfiles(createRejectedTestCommandClient(new Error('profiles unavailable')))

    expect(await screen.findByText('profiles unavailable')).toBeInTheDocument()
  })

  it('returns ready profiles from the backend', async () => {
    renderUseAgentProfiles({
      listAgentProfiles: vi.fn().mockResolvedValue({
        profiles: [
          {
            contextMode: 'minimal',
            defaultWorkspaceIsolation: 'read_only',
            description: 'Reviewer',
            id: 'reviewer',
            maxDepth: 1,
            maxTurns: 8,
            memoryScope: 'read_only',
            role: 'Reviewer',
            sandboxInheritance: 'inherit_parent',
            scope: 'builtin',
            toolBlocklist: [],
          },
        ],
      }),
    } as unknown as CommandClient)

    expect(await screen.findByText('reviewer')).toBeInTheDocument()
  })
})
