import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'

import type { DaemonClient } from '@/shared/daemon/client'
import { DaemonClientProvider } from '@/shared/tauri/react'

import { MemoryInbox } from './MemoryInbox'
import { DEFAULT_MEMORY_TENANT_ID } from './memory-types'

const workspaceRoot = '/workspace/active'
const sessionId = '01HZ0000000000000000000001'
const runId = '01HZ0000000000000000000002'

function candidate(id: string, messageId: string, content: string, hashByte: number) {
  return {
    created_at: '2026-07-12T00:00:00Z',
    evidence: {
      content_hash: Array.from({ length: 32 }, () => hashByte),
      message_id: messageId,
      origin: {
        assistant_message: {
          message_id: messageId,
          run_id: runId,
          session_id: sessionId,
        },
      },
      run_id: runId,
      session_id: sessionId,
      source: 'agent_derived',
    },
    id,
    operation: 'create',
    proposed_record: {
      content,
      kind: 'project_fact',
      metadata: { source_trust: 1, tags: ['project'], ttl: null },
      visibility: 'tenant',
    },
    state: 'proposed',
  }
}

function renderInbox(daemonClient: DaemonClient) {
  const queryClient = new QueryClient({
    defaultOptions: {
      mutations: { retry: false },
      queries: { retry: false },
    },
  })

  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <DaemonClientProvider client={daemonClient}>
        <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
      </DaemonClientProvider>
    )
  }

  return render(
    <Wrapper>
      <MemoryInbox workspaceRoot={workspaceRoot} />
    </Wrapper>,
  )
}

describe('MemoryInbox', () => {
  it('sends only candidate IDs and merged content so the daemon derives authoritative evidence', async () => {
    const first = candidate(
      '01HZ0000000000000000000011',
      '01HZ0000000000000000000021',
      'first fact',
      1,
    )
    const second = candidate(
      '01HZ0000000000000000000012',
      '01HZ0000000000000000000022',
      'second fact',
      2,
    )
    const mergeMemoryCandidate = vi.fn().mockResolvedValue({
      candidate_ids: [first.id, second.id],
      memory_id: '01HZ0000000000000000000031',
      type: 'memory_candidates_merged',
    })
    const daemonClient = {
      approveMemoryCandidate: vi.fn(),
      listMemoryCandidates: vi.fn().mockResolvedValue({
        candidates: [first, second],
        type: 'memory_candidates',
      }),
      mergeMemoryCandidate,
      rejectMemoryCandidate: vi.fn(),
    } as unknown as DaemonClient
    const user = userEvent.setup()

    renderInbox(daemonClient)
    const checkboxes = await screen.findAllByRole('checkbox', { name: 'Select candidate' })
    await user.click(checkboxes[0])
    await user.click(checkboxes[1])
    await user.click(screen.getByRole('button', { name: 'Merge' }))

    await waitFor(() => {
      expect(mergeMemoryCandidate).toHaveBeenCalledWith(workspaceRoot, {
        candidate_ids: [first.id, second.id],
        merged_record: {
          content: 'first fact\n\nsecond fact',
          expires_at: undefined,
          kind: 'project_fact',
          metadata: { source_trust: 1, tags: ['project'], ttl: null },
          visibility: 'tenant',
        },
        tenant_id: DEFAULT_MEMORY_TENANT_ID,
      })
    })
    expect(await screen.findByText('Candidates merged.')).toBeInTheDocument()
  })
})
