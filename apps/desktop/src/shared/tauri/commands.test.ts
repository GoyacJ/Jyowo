import { describe, expect, it, vi } from 'vitest'

import {
  cancelRun,
  createInvokeCommandClient,
  deleteMcpServer,
  deleteMemoryItem,
  exportMemoryItems,
  exportSupportBundle,
  getAppInfo,
  getContextSnapshot,
  getConversation,
  getHarnessHealthcheck,
  getMemoryItem,
  getReplayTimeline,
  listActivity,
  listArtifacts,
  listConversations,
  listEvalCases,
  listMcpServers,
  listMemoryItems,
  resolvePermission,
  runEvalCase,
  saveMcpServer,
  saveProviderSettings,
  startRun,
  TauriCommandPayloadError,
  updateMemoryItem,
  validateProviderSettings,
} from './commands'
import { getCommandErrorMessage } from './errors'
import { createMockCommandClient } from './mock-client'

describe('CommandClient', () => {
  it('normalizes get_app_info through Zod validation', async () => {
    const invoke = vi.fn().mockResolvedValue({
      name: 'Jyowo',
      version: '0.1.0',
      shell: 'tauri2-react',
      harness: {
        sdkCrate: 'jyowo_harness_sdk',
        mode: 'in-process',
      },
    })
    const client = createInvokeCommandClient(invoke)

    await expect(getAppInfo(client)).resolves.toMatchObject({
      name: 'Jyowo',
      shell: 'tauri2-react',
      harness: {
        sdkCrate: 'jyowo_harness_sdk',
      },
    })
    expect(invoke).toHaveBeenCalledWith('get_app_info')
  })

  it('normalizes harness_healthcheck through Zod validation', async () => {
    const invoke = vi.fn().mockResolvedValue({
      status: 'available',
      sdkCrate: 'jyowo_harness_sdk',
    })
    const client = createInvokeCommandClient(invoke)

    await expect(getHarnessHealthcheck(client)).resolves.toEqual({
      status: 'available',
      sdkCrate: 'jyowo_harness_sdk',
    })
    expect(invoke).toHaveBeenCalledWith('harness_healthcheck')
  })

  it('throws a schema error for invalid IPC payloads', async () => {
    const client = createInvokeCommandClient(vi.fn().mockResolvedValue({ name: 'Jyowo' }))

    await expect(getAppInfo(client)).rejects.toThrow(TauriCommandPayloadError)
  })

  it('throws a schema error when system command payloads include unknown fields', async () => {
    const client = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        debugToken: 'should-not-cross-boundary',
        name: 'Jyowo',
        version: '0.1.0',
        shell: 'tauri2-react',
        harness: {
          sdkCrate: 'jyowo_harness_sdk',
          mode: 'in-process',
        },
      }),
    )

    await expect(getAppInfo(client)).rejects.toThrow(TauriCommandPayloadError)
  })

  it('formats object-shaped Tauri command errors through their message', () => {
    expect(
      getCommandErrorMessage({
        code: 'RUNTIME_OPERATION_FAILED',
        message: 'conversation read failed',
      }),
    ).toBe('conversation read failed')
    expect(getCommandErrorMessage({ code: 'RUNTIME_OPERATION_FAILED' })).toBe(
      'Unknown command error',
    )
  })

  it('supports mock clients outside the Tauri runtime', async () => {
    const client = createMockCommandClient()

    await expect(getAppInfo(client)).resolves.toMatchObject({
      name: 'Jyowo',
      shell: 'tauri2-react',
    })
    await expect(getHarnessHealthcheck(client)).resolves.toEqual({
      status: 'available',
      sdkCrate: 'jyowo_harness_sdk',
    })
  })

  it('models conversation list and detail commands through Zod validation', async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === 'list_conversations') {
        return {
          conversations: [
            {
              id: 'conversation-001',
              title: 'Build the desktop foundation',
              updatedAt: '2026-06-17T00:00:00.000Z',
            },
          ],
        }
      }

      return {
        conversation: {
          id: 'conversation-001',
          messages: [
            {
              author: 'user',
              body: 'Restore the prototype',
              id: 'message-001',
              timestamp: '2026-06-17T00:00:00.000Z',
            },
          ],
          title: 'Build the desktop foundation',
          updatedAt: '2026-06-17T00:00:00.000Z',
        },
      }
    })
    const client = createInvokeCommandClient(invoke)

    await expect(listConversations(client)).resolves.toEqual({
      conversations: [
        {
          id: 'conversation-001',
          title: 'Build the desktop foundation',
          updatedAt: '2026-06-17T00:00:00.000Z',
        },
      ],
    })
    await expect(getConversation('conversation-001', client)).resolves.toMatchObject({
      conversation: {
        id: 'conversation-001',
        messages: [{ author: 'user', body: 'Restore the prototype' }],
      },
    })
    expect(invoke).toHaveBeenCalledWith('list_conversations')
    expect(invoke).toHaveBeenCalledWith('get_conversation', { conversationId: 'conversation-001' })
  })

  it('models run and permission intent commands without exposing generic execute', async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === 'start_run') {
        return { runId: 'run-001', status: 'started' }
      }

      if (command === 'cancel_run') {
        return { runId: 'run-001', status: 'cancelled' }
      }

      return { decision: 'approve', requestId: '01HZ0000000000000000000001', status: 'resolved' }
    })
    const client = createInvokeCommandClient(invoke)

    await expect(
      startRun(
        {
          conversationId: 'conversation-001',
          prompt: 'Continue implementation',
        },
        client,
      ),
    ).resolves.toEqual({ runId: 'run-001', status: 'started' })
    await expect(cancelRun('run-001', client)).resolves.toEqual({
      runId: 'run-001',
      status: 'cancelled',
    })
    await expect(
      resolvePermission({ decision: 'approve', requestId: '01HZ0000000000000000000001' }, client),
    ).resolves.toEqual({
      decision: 'approve',
      requestId: '01HZ0000000000000000000001',
      status: 'resolved',
    })

    expect(invoke).toHaveBeenCalledWith('start_run', {
      conversationId: 'conversation-001',
      prompt: 'Continue implementation',
    })
    expect(invoke).toHaveBeenCalledWith('cancel_run', { runId: 'run-001' })
    expect(invoke).toHaveBeenCalledWith('resolve_permission', {
      decision: 'approve',
      requestId: '01HZ0000000000000000000001',
    })
    expect(invoke).not.toHaveBeenCalledWith('execute', expect.anything())
  })

  it('validates permission decisions before invoking Tauri', async () => {
    const invoke = vi.fn()
    const client = createInvokeCommandClient(invoke)

    await expect(
      resolvePermission(
        { decision: 'allow', requestId: '01HZ0000000000000000000001' } as unknown as Parameters<
          typeof resolvePermission
        >[0],
        client,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
    await expect(
      resolvePermission({ decision: 'approve', requestId: ' ' }, client),
    ).rejects.toThrow(TauriCommandPayloadError)
    await expect(
      resolvePermission({ decision: 'approve', requestId: '01hz0000000000000000000001' }, client),
    ).rejects.toThrow(TauriCommandPayloadError)
    expect(invoke).not.toHaveBeenCalled()
  })

  it('models activity and context snapshot commands through parsed payloads', async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === 'list_activity') {
        return {
          events: [
            {
              id: 'evt-001',
              payload: { sessionId: 'session-001' },
              runId: 'run-001',
              sequence: 1,
              source: 'engine',
              timestamp: '2026-06-17T00:00:00.000Z',
              type: 'run.started',
              visibility: 'public',
            },
          ],
        }
      }

      return {
        activeArtifact: 'App shell',
        decisions: [{ detail: 'Before runtime integration', title: 'Review IPC boundary' }],
        files: [{ label: 'apps/desktop/src/shared/tauri/commands.ts' }],
        nextActions: ['Add Rust command handlers'],
        path: 'workspace://local',
        project: 'Jyowo',
      }
    })
    const client = createInvokeCommandClient(invoke)

    await expect(
      listActivity({ conversationId: 'conversation-001', runId: 'run-001' }, client),
    ).resolves.toMatchObject({
      events: [{ id: 'evt-001', type: 'run.started' }],
    })
    await expect(
      getContextSnapshot({ conversationId: 'conversation-001' }, client),
    ).resolves.toMatchObject({
      project: 'Jyowo',
      files: [{ label: 'apps/desktop/src/shared/tauri/commands.ts' }],
    })
    expect(invoke).toHaveBeenCalledWith('list_activity', {
      conversationId: 'conversation-001',
      runId: 'run-001',
    })
    expect(invoke).toHaveBeenCalledWith('get_context_snapshot', {
      conversationId: 'conversation-001',
    })
  })

  it('models replay and support bundle commands without exposing unredacted payloads', async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === 'get_replay_timeline') {
        return {
          events: [
            {
              id: 'evt-redacted',
              payload: { outputSummary: '[REDACTED]', toolUseId: 'tool-001' },
              runId: 'run-001',
              sequence: 1,
              source: 'tool',
              timestamp: '2026-06-17T00:00:00.000Z',
              type: 'tool.completed',
              visibility: 'redacted',
            },
            {
              id: 'evt-withheld',
              runId: 'run-001',
              sequence: 2,
              source: 'tool',
              timestamp: '2026-06-17T00:00:01.000Z',
              type: 'tool.completed',
              visibility: 'withheld',
            },
          ],
          replayed: true,
        }
      }

      return {
        bundlePath: '.jyowo/runtime/exports/support-bundle-20260617T000000.000Z.json',
        eventCount: 2,
        exportedAt: '2026-06-17T00:00:00.000Z',
        jsonlPath: '.jyowo/runtime/exports/events-20260617T000000.000Z.jsonl',
        markdownPath: '.jyowo/runtime/exports/support-report-20260617T000000.000Z.md',
        redacted: true,
      }
    })
    const client = createInvokeCommandClient(invoke)

    await expect(
      getReplayTimeline({ conversationId: 'conversation-001' }, client),
    ).resolves.toEqual({
      events: [
        {
          id: 'evt-redacted',
          payload: { outputSummary: '[REDACTED]', toolUseId: 'tool-001' },
          runId: 'run-001',
          sequence: 1,
          source: 'tool',
          timestamp: '2026-06-17T00:00:00.000Z',
          type: 'tool.completed',
          visibility: 'redacted',
        },
        {
          id: 'evt-withheld',
          runId: 'run-001',
          sequence: 2,
          source: 'tool',
          timestamp: '2026-06-17T00:00:01.000Z',
          type: 'tool.completed',
          visibility: 'withheld',
        },
      ],
      replayed: true,
    })
    await expect(
      exportSupportBundle({ conversationId: 'conversation-001' }, client),
    ).resolves.toEqual({
      bundlePath: '.jyowo/runtime/exports/support-bundle-20260617T000000.000Z.json',
      eventCount: 2,
      exportedAt: '2026-06-17T00:00:00.000Z',
      jsonlPath: '.jyowo/runtime/exports/events-20260617T000000.000Z.jsonl',
      markdownPath: '.jyowo/runtime/exports/support-report-20260617T000000.000Z.md',
      redacted: true,
    })
    expect(invoke).toHaveBeenCalledWith('get_replay_timeline', {
      conversationId: 'conversation-001',
    })
    expect(invoke).toHaveBeenCalledWith('export_support_bundle', {
      conversationId: 'conversation-001',
    })
  })

  it('models artifact history through parsed IPC payloads', async () => {
    const invoke = vi.fn().mockResolvedValue({
      artifacts: [
        {
          actionLabel: 'Open',
          description: 'Generated implementation plan and app shell review output.',
          id: 'artifact-foundation-plan',
          kind: 'markdown',
          preview: '# Foundation review',
          sourceMessageId: 'message-002',
          sourceRunId: 'run-001',
          status: 'ready',
          title: 'Foundation implementation review',
        },
      ],
    })
    const client = createInvokeCommandClient(invoke)

    await expect(listArtifacts(client)).resolves.toEqual({
      artifacts: [
        {
          actionLabel: 'Open',
          description: 'Generated implementation plan and app shell review output.',
          id: 'artifact-foundation-plan',
          kind: 'markdown',
          preview: '# Foundation review',
          sourceMessageId: 'message-002',
          sourceRunId: 'run-001',
          status: 'ready',
          title: 'Foundation implementation review',
        },
      ],
    })
    expect(invoke).toHaveBeenCalledWith('list_artifacts')
  })

  it('accepts artifact payloads with missing optional preview and source message fields', async () => {
    const client = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        artifacts: [
          {
            actionLabel: 'Open',
            description: 'Generated implementation plan',
            id: 'artifact-without-preview',
            kind: 'markdown',
            sourceRunId: 'run-001',
            status: 'ready',
            title: 'Generated output',
          },
        ],
      }),
    )

    await expect(listArtifacts(client)).resolves.toEqual({
      artifacts: [
        {
          actionLabel: 'Open',
          description: 'Generated implementation plan',
          id: 'artifact-without-preview',
          kind: 'markdown',
          sourceRunId: 'run-001',
          status: 'ready',
          title: 'Generated output',
        },
      ],
    })
  })

  it('rejects artifact payloads with unknown fields or oversized previews', async () => {
    const withUnknownField = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        artifacts: [
          {
            actionLabel: 'Open',
            description: 'Generated implementation plan',
            id: 'artifact-foundation-plan',
            kind: 'markdown',
            rawPath: '/tmp/secret-output.md',
            sourceRunId: 'run-001',
            status: 'ready',
            title: 'Foundation implementation review',
          },
        ],
      }),
    )
    const withLargePreview = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        artifacts: [
          {
            actionLabel: 'Open',
            description: 'Generated implementation plan',
            id: 'artifact-foundation-plan',
            kind: 'markdown',
            preview: 'x'.repeat(16 * 1024 + 1),
            sourceRunId: 'run-001',
            status: 'ready',
            title: 'Foundation implementation review',
          },
        ],
      }),
    )
    const withLargeMultibytePreview = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        artifacts: [
          {
            actionLabel: 'Open',
            description: 'Generated implementation plan',
            id: 'artifact-foundation-plan',
            kind: 'markdown',
            preview: '界'.repeat(6000),
            sourceRunId: 'run-001',
            status: 'ready',
            title: 'Foundation implementation review',
          },
        ],
      }),
    )

    await expect(listArtifacts(withUnknownField)).rejects.toThrow(TauriCommandPayloadError)
    await expect(listArtifacts(withLargePreview)).rejects.toThrow(TauriCommandPayloadError)
    await expect(listArtifacts(withLargeMultibytePreview)).rejects.toThrow(TauriCommandPayloadError)
  })

  it('models eval lab commands through parsed support-workflow payloads', async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === 'list_eval_cases') {
        return {
          cases: [
            {
              id: 'regression-smoke',
              lastRun: {
                completedAt: '2026-06-17T00:00:00.000Z',
                failed: 0,
                passed: 3,
                status: 'passed',
              },
              title: 'Regression smoke',
            },
          ],
        }
      }

      return {
        case: {
          id: 'regression-smoke',
          lastRun: {
            completedAt: '2026-06-17T00:00:01.000Z',
            failed: 0,
            passed: 4,
            status: 'passed',
          },
          title: 'Regression smoke',
        },
        status: 'completed',
      }
    })
    const client = createInvokeCommandClient(invoke)

    await expect(listEvalCases(client)).resolves.toEqual({
      cases: [
        {
          id: 'regression-smoke',
          lastRun: {
            completedAt: '2026-06-17T00:00:00.000Z',
            failed: 0,
            passed: 3,
            status: 'passed',
          },
          title: 'Regression smoke',
        },
      ],
    })
    await expect(runEvalCase('regression-smoke', client)).resolves.toMatchObject({
      case: {
        id: 'regression-smoke',
        lastRun: { passed: 4, status: 'passed' },
      },
      status: 'completed',
    })
    expect(invoke).toHaveBeenCalledWith('list_eval_cases')
    expect(invoke).toHaveBeenCalledWith('run_eval_case', { caseId: 'regression-smoke' })
  })

  it('rejects unredacted support bundle exports at the IPC boundary', async () => {
    const client = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        bundlePath: '.jyowo/runtime/exports/support-bundle.json',
        eventCount: 1,
        exportedAt: '2026-06-17T00:00:00.000Z',
        jsonlPath: '.jyowo/runtime/exports/events.jsonl',
        markdownPath: '.jyowo/runtime/exports/report.md',
        redacted: false,
      }),
    )

    await expect(
      exportSupportBundle({ conversationId: 'conversation-001', runId: 'run-001' }, client),
    ).rejects.toThrow(TauriCommandPayloadError)
  })

  it('rejects replay and support bundle requests without a conversation id', async () => {
    const invoke = vi.fn()
    const client = createInvokeCommandClient(invoke)

    await expect(
      getReplayTimeline(
        { runId: 'run-001' } as unknown as Parameters<typeof getReplayTimeline>[0],
        client,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
    await expect(
      exportSupportBundle(
        { runId: 'run-001' } as unknown as Parameters<typeof exportSupportBundle>[0],
        client,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
    expect(invoke).not.toHaveBeenCalled()
  })

  it('rejects support bundle paths outside workspace exports', async () => {
    const client = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        bundlePath: '/tmp/support-bundle.json',
        eventCount: 1,
        exportedAt: '2026-06-17T00:00:00.000Z',
        jsonlPath: '.jyowo/runtime/exports/events.jsonl',
        markdownPath: '.jyowo/runtime/exports/report.md',
        redacted: true,
      }),
    )

    await expect(
      exportSupportBundle({ conversationId: 'conversation-001' }, client),
    ).rejects.toThrow(TauriCommandPayloadError)
  })

  it('throws schema errors for invalid conversation IPC payloads', async () => {
    const client = createInvokeCommandClient(
      vi
        .fn()
        .mockResolvedValue({ conversations: [{ id: '', title: '', updatedAt: 'not-a-date' }] }),
    )

    await expect(listConversations(client)).rejects.toThrow(TauriCommandPayloadError)
  })

  it('rejects invalid command args before invoking Tauri', async () => {
    const invoke = vi.fn()
    const client = createInvokeCommandClient(invoke)

    await expect(startRun({ conversationId: '', prompt: '' }, client)).rejects.toThrow(
      TauriCommandPayloadError,
    )
    expect(invoke).not.toHaveBeenCalled()
  })

  it('supports explicit mock behavior for conversation commands', async () => {
    const client = createMockCommandClient()

    await expect(listConversations(client)).resolves.toHaveProperty('conversations')
    await expect(getConversation('conversation-001', client)).resolves.toHaveProperty(
      'conversation.id',
      'conversation-001',
    )
    await expect(
      startRun({ conversationId: 'conversation-001', prompt: 'Run' }, client),
    ).resolves.toHaveProperty('status', 'started')
    await expect(cancelRun('run-001', client)).resolves.toHaveProperty('status', 'cancelled')
    await expect(
      resolvePermission({ decision: 'deny', requestId: '01HZ0000000000000000000001' }, client),
    ).resolves.toHaveProperty('decision', 'deny')
    await expect(
      listActivity({ conversationId: 'conversation-001' }, client),
    ).resolves.toHaveProperty('events')
    await expect(
      getContextSnapshot({ conversationId: 'conversation-001' }, client),
    ).resolves.toHaveProperty('project')
  })

  it('validates and saves provider settings without returning raw keys', async () => {
    const providerToken = 'provider-test-token'
    const invoke = vi.fn(async (command: string) => {
      if (command === 'validate_provider_settings') {
        return {
          modelId: 'gpt-4o-mini',
          providerId: 'openai',
          status: 'accepted',
        }
      }

      return {
        modelId: 'gpt-4o-mini',
        providerId: 'openai',
        secretRef: 'provider/workspace-local/openai/default',
        status: 'saved',
      }
    })
    const client = createInvokeCommandClient(invoke)

    await expect(
      validateProviderSettings(
        {
          modelId: 'gpt-4o-mini',
          providerId: 'openai',
        },
        client,
      ),
    ).resolves.toEqual({
      modelId: 'gpt-4o-mini',
      providerId: 'openai',
      status: 'accepted',
    })
    await expect(
      saveProviderSettings(
        {
          apiKey: providerToken,
          modelId: 'gpt-4o-mini',
          providerId: 'openai',
        },
        client,
      ),
    ).resolves.toEqual({
      modelId: 'gpt-4o-mini',
      providerId: 'openai',
      secretRef: 'provider/workspace-local/openai/default',
      status: 'saved',
    })

    expect(JSON.stringify(invoke.mock.results)).not.toContain(providerToken)
    expect(invoke).toHaveBeenCalledWith('validate_provider_settings', {
      modelId: 'gpt-4o-mini',
      providerId: 'openai',
    })
    expect(invoke).toHaveBeenCalledWith('save_provider_settings', {
      apiKey: providerToken,
      modelId: 'gpt-4o-mini',
      providerId: 'openai',
    })
  })

  it('rejects invalid provider settings before invoking Tauri', async () => {
    const invoke = vi.fn()
    const client = createInvokeCommandClient(invoke)

    await expect(
      saveProviderSettings(
        {
          apiKey: '',
          modelId: '',
          providerId: 'unknown',
        } as unknown as Parameters<typeof saveProviderSettings>[0],
        client,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
    expect(invoke).not.toHaveBeenCalled()
  })

  it('models MCP server commands without exposing raw secret fields', async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === 'list_mcp_servers') {
        return {
          servers: [
            {
              displayName: 'Workspace GitHub',
              exposedToolCount: 2,
              id: 'github',
              origin: 'workspace',
              scope: 'global',
              status: 'ready',
              transport: 'stdio',
            },
          ],
        }
      }

      if (command === 'delete_mcp_server') {
        return {
          id: 'github',
          status: 'deleted',
        }
      }

      return {
        server: {
          displayName: 'Workspace GitHub',
          exposedToolCount: 0,
          id: 'github',
          origin: 'workspace',
          scope: 'global',
          status: 'configured',
          transport: 'stdio',
        },
      }
    })
    const client = createInvokeCommandClient(invoke)

    await expect(listMcpServers(client)).resolves.toEqual({
      servers: [
        {
          displayName: 'Workspace GitHub',
          exposedToolCount: 2,
          id: 'github',
          origin: 'workspace',
          scope: 'global',
          status: 'ready',
          transport: 'stdio',
        },
      ],
    })
    await expect(
      saveMcpServer(
        {
          displayName: 'Workspace GitHub',
          id: 'github',
          scope: 'global',
          transport: {
            args: ['mcp-server'],
            command: 'node',
            kind: 'stdio',
          },
        },
        client,
      ),
    ).resolves.toHaveProperty('server.status', 'configured')
    await expect(deleteMcpServer('github', client)).resolves.toEqual({
      id: 'github',
      status: 'deleted',
    })

    expect(JSON.stringify(invoke.mock.results)).not.toContain('Authorization')
    expect(invoke).toHaveBeenCalledWith('list_mcp_servers')
    expect(invoke).toHaveBeenCalledWith('save_mcp_server', {
      displayName: 'Workspace GitHub',
      id: 'github',
      scope: 'global',
      transport: {
        args: ['mcp-server'],
        command: 'node',
        kind: 'stdio',
      },
    })
    expect(invoke).toHaveBeenCalledWith('delete_mcp_server', { id: 'github' })
  })

  it('rejects invalid MCP server args before invoking Tauri', async () => {
    const invoke = vi.fn()
    const client = createInvokeCommandClient(invoke)

    await expect(
      saveMcpServer(
        {
          displayName: '',
          id: 'bad id',
          scope: 'global',
          transport: {
            args: [],
            command: '',
            kind: 'stdio',
          },
        } as unknown as Parameters<typeof saveMcpServer>[0],
        client,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
    await expect(deleteMcpServer('', client)).rejects.toThrow(TauriCommandPayloadError)
    expect(invoke).not.toHaveBeenCalled()
  })

  it('models memory browser commands through parsed payloads without generic execution', async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === 'list_memory_items') {
        return {
          items: [
            {
              contentPreview: 'Prefers concise Chinese responses',
              id: '01HZ0000000000000000000001',
              kind: 'user_preference',
              source: 'user_input',
              tags: ['tone'],
              updatedAt: '2026-06-17T00:00:00.000Z',
              visibility: 'tenant',
            },
          ],
        }
      }

      if (command === 'get_memory_item' || command === 'update_memory_item') {
        return {
          item: {
            accessCount: 0,
            confidence: 1,
            content: 'Prefers concise Chinese responses',
            createdAt: '2026-06-17T00:00:00.000Z',
            id: '01HZ0000000000000000000001',
            kind: 'user_preference',
            source: 'user_input',
            tags: ['tone'],
            updatedAt: '2026-06-17T00:00:00.000Z',
            visibility: 'tenant',
          },
        }
      }

      if (command === 'delete_memory_item') {
        return {
          id: '01HZ0000000000000000000001',
          status: 'deleted',
        }
      }

      return {
        exportedAt: '2026-06-17T00:00:00.000Z',
        format: 'json',
        itemCount: 1,
        path: '.jyowo/runtime/exports/memory-20260617T000000.000Z.json',
      }
    })
    const client = createInvokeCommandClient(invoke)

    await expect(listMemoryItems(client)).resolves.toHaveProperty('items.0.visibility', 'tenant')
    await expect(getMemoryItem('01HZ0000000000000000000001', client)).resolves.toHaveProperty(
      'item.content',
      'Prefers concise Chinese responses',
    )
    await expect(
      updateMemoryItem(
        {
          content: '  Prefers terse Chinese responses\n',
          id: '01HZ0000000000000000000001',
        },
        client,
      ),
    ).resolves.toHaveProperty('item.id', '01HZ0000000000000000000001')
    await expect(deleteMemoryItem('01HZ0000000000000000000001', client)).resolves.toEqual({
      id: '01HZ0000000000000000000001',
      status: 'deleted',
    })
    await expect(exportMemoryItems(client)).resolves.toEqual({
      exportedAt: '2026-06-17T00:00:00.000Z',
      format: 'json',
      itemCount: 1,
      path: '.jyowo/runtime/exports/memory-20260617T000000.000Z.json',
    })

    expect(invoke).toHaveBeenCalledWith('list_memory_items')
    expect(invoke).toHaveBeenCalledWith('get_memory_item', {
      id: '01HZ0000000000000000000001',
    })
    expect(invoke).toHaveBeenCalledWith('update_memory_item', {
      content: '  Prefers terse Chinese responses\n',
      id: '01HZ0000000000000000000001',
    })
    expect(invoke).toHaveBeenCalledWith('delete_memory_item', {
      id: '01HZ0000000000000000000001',
    })
    expect(invoke).toHaveBeenCalledWith('export_memory_items')
    expect(invoke).not.toHaveBeenCalledWith('execute', expect.anything())
  })

  it('rejects invalid memory command args before invoking Tauri', async () => {
    const invoke = vi.fn()
    const client = createInvokeCommandClient(invoke)

    await expect(getMemoryItem('', client)).rejects.toThrow(TauriCommandPayloadError)
    await expect(deleteMemoryItem('', client)).rejects.toThrow(TauriCommandPayloadError)
    await expect(
      updateMemoryItem({ content: '', id: '01HZ0000000000000000000001' }, client),
    ).rejects.toThrow(TauriCommandPayloadError)
    expect(invoke).not.toHaveBeenCalled()
  })
})
