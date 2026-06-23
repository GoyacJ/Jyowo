import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { CommandClient, ListArtifactsResponse } from '@/shared/tauri/commands'
import { createMockCommandClient, createRejectedCommandClient } from '@/shared/tauri/mock-client'
import { CommandClientProvider } from '@/shared/tauri/react'

import { ArtifactsPage } from './ArtifactsPage'

const artifactPreviewProps = vi.hoisted(() => [] as Array<{ content?: string; state: string }>)

vi.mock('./ArtifactPreview', async (importOriginal) => {
  const original = await importOriginal<typeof import('./ArtifactPreview')>()

  return {
    ...original,
    ArtifactPreview: (props: import('./ArtifactPreview').ArtifactPreviewProps) => {
      artifactPreviewProps.push({ content: props.content, state: props.state })

      return original.ArtifactPreview(props)
    },
  }
})

const artifacts: ListArtifactsResponse = {
  artifacts: [
    {
      actionLabel: 'Open',
      description: 'Generated implementation plan and app shell review output.',
      id: 'artifact-foundation-plan',
      kind: 'markdown',
      preview: '# Foundation review',
      status: 'ready',
      title: 'Foundation implementation review',
    },
    {
      actionLabel: 'Open',
      description: 'Generated verification checklist.',
      id: 'artifact-verification',
      kind: 'markdown',
      preview: '# Verification',
      status: 'pending',
      title: 'Verification notes',
    },
  ],
}

function renderArtifactsPage(commandClient: CommandClient = createMockCommandClient()) {
  const queryClient = new QueryClient({
    defaultOptions: {
      mutations: { retry: false },
      queries: { retry: false },
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
    queryClient,
    ...render(<ArtifactsPage />, { wrapper: Wrapper }),
  }
}

describe('ArtifactsPage', () => {
  beforeEach(() => {
    artifactPreviewProps.length = 0
  })

  it('loads artifact history from the command client and switches previews', async () => {
    const commandClient = createMockCommandClient({ artifacts })
    const listArtifacts = vi.fn(commandClient.listArtifacts)
    const trackedClient = {
      ...commandClient,
      listArtifacts,
    } satisfies CommandClient

    renderArtifactsPage(trackedClient)

    expect(
      await screen.findByRole('article', { name: 'Foundation implementation review' }),
    ).toBeInTheDocument()
    const history = screen.getByRole('region', { name: 'Artifact history' })
    expect(
      within(history).getByRole('article', { name: 'Foundation implementation review' }),
    ).toBeInTheDocument()
    expect(screen.getByText('# Foundation review')).toBeInTheDocument()

    fireEvent.click(
      within(within(history).getByRole('article', { name: 'Verification notes' })).getByRole(
        'button',
        { name: 'Open' },
      ),
    )

    await waitFor(() => {
      expect(listArtifacts).toHaveBeenCalled()
    })
    expect(screen.getByText('# Verification')).toBeInTheDocument()
  })

  it('renders empty, loading, and error states without raw backend details', async () => {
    const { unmount } = renderArtifactsPage(createMockCommandClient({ delayMs: 10 }))

    expect(screen.getByText('Loading artifacts')).toBeInTheDocument()
    expect(
      await screen.findByRole('article', { name: 'Desktop foundation created' }),
    ).toBeInTheDocument()

    unmount()

    const { unmount: unmountEmpty } = renderArtifactsPage(
      createMockCommandClient({ artifacts: { artifacts: [] } }),
    )

    expect(await screen.findByText('No artifacts for this conversation.')).toBeInTheDocument()
    expect(screen.getByText('No artifact selected.')).toBeInTheDocument()

    unmountEmpty()

    renderArtifactsPage(
      createRejectedCommandClient(new Error('artifact failed with Authorization Bearer secret')),
    )

    expect(await screen.findByText('Artifact history could not be loaded.')).toBeInTheDocument()
    expect(screen.getByText('Artifact preview unavailable.')).toBeInTheDocument()
    expect(screen.queryByText(/Authorization Bearer/)).not.toBeInTheDocument()
  })

  it('does not keep stale preview visible after artifact refetch fails', async () => {
    const listArtifacts = vi
      .fn()
      .mockResolvedValueOnce(artifacts)
      .mockRejectedValueOnce(new Error('artifact failed with Authorization Bearer secret'))
    const commandClient = {
      ...createMockCommandClient(),
      listArtifacts,
    } satisfies CommandClient
    const { queryClient } = renderArtifactsPage(commandClient)

    expect(await screen.findByText('# Foundation review')).toBeInTheDocument()

    await act(async () => {
      await queryClient.invalidateQueries({ queryKey: ['artifacts'] })
    })

    await waitFor(() => {
      expect(screen.getByText('Artifact history could not be loaded.')).toBeInTheDocument()
    })
    expect(screen.getByText('Artifact preview unavailable.')).toBeInTheDocument()
    expect(screen.queryByText('# Foundation review')).not.toBeInTheDocument()
    expect(artifactPreviewProps.at(-1)).toMatchObject({
      content: undefined,
      state: 'error',
    })
    expect(screen.queryByText(/Authorization Bearer/)).not.toBeInTheDocument()
  })
})
