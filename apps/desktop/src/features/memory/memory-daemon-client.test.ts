import { describe, expect, it, vi } from 'vitest'

import { createDaemonClient } from '@/shared/daemon/client'

describe('daemon memory client', () => {
  it('sends the active workspace root with memory list requests', async () => {
    const invoke = vi.fn().mockResolvedValue({
      message: { items: [], type: 'memory_items' },
      protocolVersion: 6,
      requestId: 'memory-request',
    })
    const client = createDaemonClient(
      { invoke, listen: vi.fn() },
      { requestId: () => 'memory-request' },
    )

    await client.listMemoryItems('/workspace/active')

    expect(invoke).toHaveBeenCalledWith('daemon_request', {
      frame: {
        protocolVersion: 6,
        request: { type: 'list_memory_items', workspaceRoot: '/workspace/active' },
        requestId: 'memory-request',
      },
    })
  })

  it('keeps update, delete, candidate, and settings operations on the active workspace', async () => {
    const memoryId = '01HZ0000000000000000000001'
    const memoryItem = {
      accessCount: 0,
      confidence: 1,
      content: 'updated',
      contentHash: '0'.repeat(64),
      createdAt: '2026-07-12T00:00:00Z',
      deleted: false,
      id: memoryId,
      kind: 'project_fact',
      source: 'agent_derived',
      tags: [],
      updatedAt: '2026-07-12T00:00:00Z',
      visibility: 'tenant',
    }
    const settings = {
      disable_generation_when_external_context_used: false,
      generate_memories: true,
      max_memory_bytes: 10_000,
      max_recall_chars_per_turn: 5_000,
      max_recall_records_per_turn: 20,
      use_memories: true,
    }
    const messages = [
      { item: memoryItem, type: 'memory_updated' },
      { memoryId, type: 'memory_deleted' },
      { candidates: [], type: 'memory_candidates' },
      { settings, type: 'memory_settings_updated' },
    ]
    const invoke = vi.fn().mockImplementation(async () => ({
      message: messages.shift(),
      protocolVersion: 6,
      requestId: 'memory-request',
    }))
    const client = createDaemonClient(
      { invoke, listen: vi.fn() },
      { requestId: () => 'memory-request' },
    )
    const workspaceRoot = '/workspace/active'

    await client.updateMemoryItem(workspaceRoot, { content: 'updated', id: memoryId })
    await client.deleteMemoryItem(workspaceRoot, { id: memoryId })
    await client.listMemoryCandidates(workspaceRoot, {
      limit: 50,
      tenant_id: '00000000000000000000000001',
    })
    await client.updateMemorySettings(workspaceRoot, {
      settings,
      tenant_id: '00000000000000000000000001',
    })

    expect(
      invoke.mock.calls.map(([, args]) => {
        const frame = (args as { frame: { request: { type: string; workspaceRoot: string } } })
          .frame
        return [frame.request.type, frame.request.workspaceRoot]
      }),
    ).toEqual([
      ['update_memory_item', workspaceRoot],
      ['delete_memory_item', workspaceRoot],
      ['list_memory_candidates', workspaceRoot],
      ['update_memory_settings', workspaceRoot],
    ])
  })

  it('loads model request previews from the daemon-owned workspace database', async () => {
    const preview = {
      content_hash: Array.from({ length: 32 }, () => 0),
      policy_decisions: [],
      redacted_count: 0,
      run_id: '01HZ0000000000000000000002',
      sections: [],
      session_id: '01HZ0000000000000000000001',
      token_estimate: 0,
      tool_names: [],
    }
    const invoke = vi.fn().mockResolvedValue({
      message: { preview, type: 'model_request_preview' },
      protocolVersion: 6,
      requestId: 'memory-request',
    })
    const client = createDaemonClient(
      { invoke, listen: vi.fn() },
      { requestId: () => 'memory-request' },
    )

    await client.getModelRequestPreview('/workspace/active', {
      run_id: preview.run_id,
      session_id: preview.session_id,
      tenant_id: '00000000000000000000000001',
    })

    expect(invoke).toHaveBeenCalledWith('daemon_request', {
      frame: {
        protocolVersion: 6,
        request: {
          request: {
            run_id: preview.run_id,
            session_id: preview.session_id,
            tenant_id: '00000000000000000000000001',
          },
          type: 'get_model_request_preview',
          workspaceRoot: '/workspace/active',
        },
        requestId: 'memory-request',
      },
    })
  })
})
