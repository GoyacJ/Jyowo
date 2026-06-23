import { describe, expect, it, vi } from 'vitest'

function cursor(_label: string, conversationSequence = 1) {
  return { eventId: '01ARZ3NDEKTSV4RRFFQ69G5FAV', conversationSequence }
}

const tauriListenMock = vi.hoisted(() => vi.fn())

vi.mock('@tauri-apps/api/event', () => ({
  listen: tauriListenMock,
}))

import {
  cancelRun,
  createAttachmentFromPath,
  createConversation,
  createInvokeCommandClient,
  deleteConversation,
  deleteMcpServer,
  deleteMemoryItem,
  deleteSkill,
  exportMemoryItems,
  exportSupportBundle,
  getAppInfo,
  getContextSnapshot,
  getConversation,
  getHarnessHealthcheck,
  getMemoryItem,
  getProviderConfigApiKey,
  getReplayTimeline,
  getSkillDetail,
  getSkillFile,
  importSkill,
  listActivity,
  listArtifacts,
  listConversations,
  listEvalCases,
  listMcpServers,
  listMemoryItems,
  listModelProviderCatalog,
  listProviderSettings,
  listReferenceCandidates,
  listSkills,
  requestProviderConfigApiKeyReveal,
  resolvePermission,
  runEvalCase,
  saveMcpServer,
  saveProviderSettings,
  setSkillEnabled,
  startRun,
  TauriCommandPayloadError,
  updateMemoryItem,
  validateProviderSettings,
} from './commands'
import { getCommandErrorMessage } from './errors'
import { createMockCommandClient } from './mock-client'

const validAttachmentId =
  'attachment-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'
const validBlobRef = {
  contentHash: Array.from({ length: 32 }, () => 1),
  contentType: 'text/plain',
  id: '01J00000000000000000000000',
  size: 128,
}
const openAiModelDescriptor = {
  protocol: 'responses',
  conversationCapability: {
    inputModalities: ['text'],
    outputModalities: ['text'],
    contextWindow: 128000,
    maxOutputTokens: 16384,
    streaming: true,
    toolCalling: true,
    reasoning: false,
    promptCache: false,
    structuredOutput: true,
  },
  contextWindow: 128000,
  displayName: 'GPT-5.4 mini',
  lifecycle: { kind: 'stable' },
  maxOutputTokens: 16384,
  modelId: 'gpt-5.4-mini',
  runtimeStatus: { kind: 'runnable' },
} as const

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

  it('keeps mock timeline subscription replay separate from activity defaults', async () => {
    const defaultClient = createMockCommandClient()

    await expect(
      defaultClient.subscribeConversationEvents({ conversationId: 'conversation-001' }),
    ).resolves.toMatchObject({
      conversationId: 'conversation-001',
      replayEvents: [],
      gap: false,
    })

    const streamingClient = createMockCommandClient({
      subscribeConversationEvents: {
        subscriptionId: 'subscription-stream',
        conversationId: 'conversation-001',
        replayEvents: [
          {
            id: 'evt-delta',
            conversationSequence: 1,
            payload: { text: 'streamed' },
            runId: 'run-001',
            sequence: 1,
            source: 'assistant',
            timestamp: '2026-06-17T00:00:00.000Z',
            type: 'assistant.delta',
            visibility: 'public',
          },
        ],
        cursor: cursor(''),
        gap: false,
      },
    })

    await expect(
      streamingClient.subscribeConversationEvents({ conversationId: 'conversation-001' }),
    ).resolves.toMatchObject({
      subscriptionId: 'subscription-stream',
      replayEvents: [{ id: 'evt-delta' }],
      cursor: cursor(''),
    })
  })

  it('models conversation list and detail commands through Zod validation', async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === 'list_conversations') {
        return {
          conversations: [
            {
              id: 'conversation-001',
              isEmpty: false,
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
          modelConfigId: null,
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
          isEmpty: false,
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

  it('accepts empty conversation summaries when optional preview is omitted', async () => {
    const client = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        conversations: [
          {
            id: 'conversation-empty-001',
            isEmpty: true,
            title: 'New conversation',
            updatedAt: '2026-06-17T00:00:00.000Z',
          },
        ],
      }),
    )

    await expect(listConversations(client)).resolves.toEqual({
      conversations: [
        {
          id: 'conversation-empty-001',
          isEmpty: true,
          title: 'New conversation',
          updatedAt: '2026-06-17T00:00:00.000Z',
        },
      ],
    })
  })

  it('rejects unsafe conversation snapshot message bodies', async () => {
    const snapshot = (body: string) => ({
      conversation: {
        id: 'conversation-001',
        messages: [
          {
            author: 'assistant',
            body,
            id: 'message-001',
            timestamp: '2026-06-17T00:00:00.000Z',
          },
        ],
        modelConfigId: null,
        title: 'Build the desktop foundation',
        updatedAt: '2026-06-17T00:00:00.000Z',
      },
    })

    await expect(
      getConversation(
        'conversation-001',
        createInvokeCommandClient(vi.fn().mockResolvedValue(snapshot('/Users/goya/.ssh/config'))),
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
    await expect(
      getConversation(
        'conversation-001',
        createInvokeCommandClient(vi.fn().mockResolvedValue(snapshot('AKIAIOSFODNN7EXAMPLE'))),
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
  })

  it('models conversation deletion through Zod validation', async () => {
    const invoke = vi.fn().mockResolvedValue({
      conversationId: 'conversation-001',
      status: 'deleted',
    })
    const client = createInvokeCommandClient(invoke)

    await expect(deleteConversation('conversation-001', client)).resolves.toEqual({
      conversationId: 'conversation-001',
      status: 'deleted',
    })
    expect(invoke).toHaveBeenCalledWith('delete_conversation', {
      conversationId: 'conversation-001',
    })
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
          attachments: [
            {
              blobRef: validBlobRef,
              id: validAttachmentId,
              mimeType: 'text/plain',
              name: 'notes.txt',
              sizeBytes: 128,
            },
          ],
          clientMessageId: '00000000-0000-4000-8000-000000000001',
          conversationId: 'conversation-001',
          contextReferences: [
            {
              kind: 'workspace_file',
              label: 'Commands',
              path: 'apps/desktop/src/shared/tauri/commands.ts',
            },
            {
              id: 'skill-review',
              kind: 'skill',
              label: 'Code review skill',
            },
            {
              id: 'builtin.grep',
              kind: 'tool',
              label: 'Search files',
            },
            {
              id: 'mcp-filesystem',
              kind: 'mcp_server',
              label: 'Filesystem MCP',
            },
          ],
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
      resolvePermission(
        {
          conversationId: 'conversation-001',
          decision: 'approve',
          requestId: '01HZ0000000000000000000001',
        },
        client,
      ),
    ).resolves.toEqual({
      decision: 'approve',
      requestId: '01HZ0000000000000000000001',
      status: 'resolved',
    })

    expect(invoke).toHaveBeenCalledWith('start_run', {
      attachments: [
        {
          blobRef: validBlobRef,
          id: validAttachmentId,
          mimeType: 'text/plain',
          name: 'notes.txt',
          sizeBytes: 128,
        },
      ],
      clientMessageId: '00000000-0000-4000-8000-000000000001',
      conversationId: 'conversation-001',
      contextReferences: [
        {
          kind: 'workspace_file',
          label: 'Commands',
          path: 'apps/desktop/src/shared/tauri/commands.ts',
        },
        {
          id: 'skill-review',
          kind: 'skill',
          label: 'Code review skill',
        },
        {
          id: 'builtin.grep',
          kind: 'tool',
          label: 'Search files',
        },
        {
          id: 'mcp-filesystem',
          kind: 'mcp_server',
          label: 'Filesystem MCP',
        },
      ],
      prompt: 'Continue implementation',
    })
    expect(invoke).toHaveBeenCalledWith('cancel_run', { runId: 'run-001' })
    expect(invoke).toHaveBeenCalledWith('resolve_permission', {
      conversationId: 'conversation-001',
      decision: 'approve',
      requestId: '01HZ0000000000000000000001',
    })
    expect(invoke).not.toHaveBeenCalledWith('execute', expect.anything())
  })

  it('models conversation event subscription commands through parsed payloads', async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === 'subscribe_conversation_events') {
        return {
          subscriptionId: 'subscription-001',
          conversationId: 'conversation-001',
          replayEvents: [
            {
              id: 'evt-replay',
              conversationSequence: 1,
              payload: { text: 'Hello' },
              runId: 'run-001',
              sequence: 1,
              source: 'assistant',
              timestamp: '2026-06-17T00:00:00.000Z',
              type: 'assistant.delta',
              visibility: 'public',
            },
          ],
          cursor: cursor(''),
          gap: false,
        }
      }

      return {
        subscriptionId: 'subscription-001',
        status: 'unsubscribed',
      }
    })
    const unlisten = vi.fn()
    let tauriEventHandler: ((event: { payload: unknown }) => void) | undefined
    tauriListenMock.mockImplementationOnce(async (_eventName, handler) => {
      tauriEventHandler = handler
      return unlisten
    })
    const client = createInvokeCommandClient(invoke)
    const batches: unknown[] = []

    await expect(
      client.subscribeConversationEvents({
        conversationId: 'conversation-001',
        afterCursor: cursor(''),
      }),
    ).resolves.toMatchObject({
      subscriptionId: 'subscription-001',
      replayEvents: [{ id: 'evt-replay' }],
      cursor: cursor(''),
    })
    const cleanup = await client.listenConversationEventBatches((batch) => {
      batches.push(batch)
    })
    tauriEventHandler?.({
      payload: {
        subscriptionId: 'subscription-001',
        conversationId: 'conversation-001',
        events: [
          {
            id: 'evt-live',
            conversationSequence: 2,
            payload: { messageId: 'message-001', body: 'Final' },
            runId: 'run-001',
            sequence: 2,
            source: 'assistant',
            timestamp: '2026-06-17T00:00:01.000Z',
            type: 'assistant.completed',
            visibility: 'public',
          },
        ],
        cursor: cursor(''),
        gap: false,
        phase: 'live',
      },
    })
    cleanup()

    await expect(client.unsubscribeConversationEvents('subscription-001')).resolves.toEqual({
      subscriptionId: 'subscription-001',
      status: 'unsubscribed',
    })
    expect(invoke).toHaveBeenCalledWith('subscribe_conversation_events', {
      conversationId: 'conversation-001',
      afterCursor: cursor(''),
    })
    expect(tauriListenMock).toHaveBeenCalledWith('conversation_event_batch', expect.any(Function))
    expect(batches).toEqual([
      expect.objectContaining({
        subscriptionId: 'subscription-001',
        events: [expect.objectContaining({ id: 'evt-live' })],
        phase: 'live',
      }),
    ])
    expect(unlisten).toHaveBeenCalledTimes(1)
    expect(invoke).toHaveBeenCalledWith('unsubscribe_conversation_events', {
      subscriptionId: 'subscription-001',
    })
  })

  it('emits mock permission requests with production-compatible ids', async () => {
    const client = createMockCommandClient()
    const permissionRequest = new Promise<string>((resolve) => {
      void client.listenConversationEventBatches((batch) => {
        const permissionEvent = batch.events.find((event) => event.type === 'permission.requested')
        if (permissionEvent?.type === 'permission.requested' && permissionEvent.payload) {
          resolve(permissionEvent.payload.requestId)
        }
      })
    })

    await client.subscribeConversationEvents({ conversationId: 'conversation-001' })
    await client.startRun({
      attachments: [],
      clientMessageId: '00000000-0000-4000-8000-000000000001',
      contextReferences: [],
      conversationId: 'conversation-001',
      prompt: 'Run local verification',
    })

    await expect(permissionRequest).resolves.toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/)
  })

  it('validates composer context command payloads before invoking Tauri', async () => {
    const invoke = vi.fn()
    const client = createInvokeCommandClient(invoke)

    await expect(
      startRun(
        {
          conversationId: 'conversation-001',
          contextReferences: [{ kind: 'workspace_file', label: '', path: 'Cargo.toml' }],
          prompt: 'Continue',
        },
        client,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
    await expect(
      startRun(
        {
          conversationId: 'conversation-001',
          intentMode: 'execute',
          prompt: 'Continue',
        } as unknown as Parameters<typeof startRun>[0],
        client,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
    await expect(
      startRun(
        {
          clientMessageId: '00000000-0000-1000-8000-000000000001',
          conversationId: 'conversation-001',
          prompt: 'Continue',
        },
        client,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
    await expect(
      startRun(
        {
          attachments: [
            {
              blobRef: {
                contentHash: [1, 2, 3],
                id: 'blob-001',
                size: 128,
              },
              id: '../escape',
              mimeType: 'text/plain',
              name: 'notes.txt',
              sizeBytes: 128,
            },
          ],
          conversationId: 'conversation-001',
          prompt: 'Continue',
        },
        client,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
    await expect(createAttachmentFromPath('', client)).rejects.toThrow(TauriCommandPayloadError)

    expect(invoke).not.toHaveBeenCalled()
  })

  it('models attachment and reference candidate commands', async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === 'create_attachment_from_path') {
        return {
          attachment: {
            blobRef: validBlobRef,
            id: validAttachmentId,
            mimeType: 'text/plain',
            name: 'notes.txt',
            sizeBytes: 128,
          },
        }
      }

      return {
        artifacts: [],
        conversations: [],
        files: [{ label: 'Cargo.toml', path: 'Cargo.toml' }],
        memories: [],
        mcpServers: [{ id: 'mcp-filesystem', label: 'Filesystem MCP' }],
        skills: [{ id: 'skill-review', label: 'Code review skill' }],
        tools: [{ id: 'builtin.grep', label: 'Search files' }],
      }
    })
    const client = createInvokeCommandClient(invoke)

    await expect(createAttachmentFromPath('/tmp/notes.txt', client)).resolves.toMatchObject({
      attachment: { id: validAttachmentId, name: 'notes.txt' },
    })
    await expect(
      listReferenceCandidates({ conversationId: 'conversation-001' }, client),
    ).resolves.toEqual({
      artifacts: [],
      conversations: [],
      files: [{ label: 'Cargo.toml', path: 'Cargo.toml' }],
      memories: [],
      mcpServers: [{ id: 'mcp-filesystem', label: 'Filesystem MCP' }],
      skills: [{ id: 'skill-review', label: 'Code review skill' }],
      tools: [{ id: 'builtin.grep', label: 'Search files' }],
    })

    expect(invoke).toHaveBeenCalledWith('create_attachment_from_path', { path: '/tmp/notes.txt' })
    expect(invoke).toHaveBeenCalledWith('list_reference_candidates', {
      conversationId: 'conversation-001',
    })
  })

  it('validates permission decisions before invoking Tauri', async () => {
    const invoke = vi.fn()
    const client = createInvokeCommandClient(invoke)

    await expect(
      resolvePermission(
        {
          conversationId: 'conversation-001',
          decision: 'allow',
          requestId: '01HZ0000000000000000000001',
        } as unknown as Parameters<typeof resolvePermission>[0],
        client,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
    await expect(
      resolvePermission(
        { conversationId: 'conversation-001', decision: 'approve', requestId: ' ' },
        client,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
    await expect(
      resolvePermission(
        {
          conversationId: 'conversation-001',
          decision: 'approve',
          requestId: '01hz0000000000000000000001',
        },
        client,
      ),
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
              conversationSequence: 1,
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
              conversationSequence: 1,
              payload: {
                outputSummary: 'Output withheld from conversation timeline.',
                toolUseId: 'tool-001',
              },
              runId: 'run-001',
              sequence: 1,
              source: 'tool',
              timestamp: '2026-06-17T00:00:00.000Z',
              type: 'tool.completed',
              visibility: 'redacted',
            },
            {
              id: 'evt-withheld',
              conversationSequence: 2,
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
          conversationSequence: 1,
          payload: {
            outputSummary: 'Output withheld from conversation timeline.',
            toolUseId: 'tool-001',
          },
          runId: 'run-001',
          sequence: 1,
          source: 'tool',
          timestamp: '2026-06-17T00:00:00.000Z',
          type: 'tool.completed',
          visibility: 'redacted',
        },
        {
          id: 'evt-withheld',
          conversationSequence: 2,
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
          status: 'ready',
          title: 'Foundation implementation review',
        },
      ],
    })
    const client = createInvokeCommandClient(invoke)

    await expect(listArtifacts({ conversationId: 'conversation-001' }, client)).resolves.toEqual({
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
      ],
    })
    expect(invoke).toHaveBeenCalledWith('list_artifacts', {
      conversationId: 'conversation-001',
    })
  })

  it('accepts artifact payloads with missing optional previews', async () => {
    const client = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        artifacts: [
          {
            actionLabel: 'Open',
            description: 'Generated implementation plan',
            id: 'artifact-without-preview',
            kind: 'markdown',
            status: 'ready',
            title: 'Generated output',
          },
        ],
      }),
    )

    await expect(listArtifacts({ conversationId: 'conversation-001' }, client)).resolves.toEqual({
      artifacts: [
        {
          actionLabel: 'Open',
          description: 'Generated implementation plan',
          id: 'artifact-without-preview',
          kind: 'markdown',
          status: 'ready',
          title: 'Generated output',
        },
      ],
    })
  })

  it('requires a conversation id when listing artifacts', async () => {
    const invoke = vi.fn()
    const client = createInvokeCommandClient(invoke)

    await expect(listArtifacts({} as never, client)).rejects.toThrow(TauriCommandPayloadError)
    expect(invoke).not.toHaveBeenCalled()
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
            status: 'ready',
            title: 'Foundation implementation review',
          },
        ],
      }),
    )
    const withSourceIds = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        artifacts: [
          {
            actionLabel: 'Open',
            description: 'Generated implementation plan',
            id: 'artifact-foundation-plan',
            kind: 'markdown',
            sourceMessageId: 'message-001',
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
            status: 'ready',
            title: 'Foundation implementation review',
          },
        ],
      }),
    )

    await expect(
      listArtifacts({ conversationId: 'conversation-001' }, withUnknownField),
    ).rejects.toThrow(TauriCommandPayloadError)
    await expect(
      listArtifacts({ conversationId: 'conversation-001' }, withSourceIds),
    ).rejects.toThrow(TauriCommandPayloadError)
    await expect(
      listArtifacts({ conversationId: 'conversation-001' }, withLargePreview),
    ).rejects.toThrow(TauriCommandPayloadError)
    await expect(
      listArtifacts({ conversationId: 'conversation-001' }, withLargeMultibytePreview),
    ).rejects.toThrow(TauriCommandPayloadError)
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

  it('rejects conversation summaries with private paths', async () => {
    const client = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        conversations: [
          {
            id: 'conversation-001',
            isEmpty: false,
            lastMessagePreview: 'read /Users/goya/.ssh/config',
            title: 'read /Users/goya/.ssh/config',
            updatedAt: '2026-06-17T00:00:00.000Z',
          },
        ],
      }),
    )

    await expect(listConversations(client)).rejects.toThrow(TauriCommandPayloadError)
  })

  it('rejects private paths adjacent to punctuation in conversation IPC payloads', async () => {
    const client = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        conversations: [
          {
            id: 'conversation-001',
            isEmpty: false,
            lastMessagePreview: 'error(path=/Users/goya/.ssh/config)',
            title: 'error(path=/Users/goya/.ssh/config)',
            updatedAt: '2026-06-17T00:00:00.000Z',
          },
        ],
      }),
    )

    await expect(listConversations(client)).rejects.toThrow(TauriCommandPayloadError)
  })

  it('rejects conversation detail titles with private paths', async () => {
    const client = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        conversation: {
          id: 'conversation-001',
          messages: [],
          modelConfigId: null,
          title: 'read /Users/goya/.ssh/config',
          updatedAt: '2026-06-17T00:00:00.000Z',
        },
      }),
    )

    await expect(getConversation('conversation-001', client)).rejects.toThrow(
      TauriCommandPayloadError,
    )
  })

  it('creates conversations through Tauri and validates the returned summary', async () => {
    const invoke = vi.fn().mockResolvedValue({
      conversation: {
        id: 'conversation-created-001',
        isEmpty: true,
        lastMessagePreview: 'Start from the composer when ready.',
        title: 'New conversation',
        updatedAt: '2026-06-17T00:00:00.000Z',
      },
    })
    const client = createInvokeCommandClient(invoke)

    await expect(createConversation(client)).resolves.toEqual({
      conversation: {
        id: 'conversation-created-001',
        isEmpty: true,
        lastMessagePreview: 'Start from the composer when ready.',
        title: 'New conversation',
        updatedAt: '2026-06-17T00:00:00.000Z',
      },
    })
    expect(invoke).toHaveBeenCalledWith('create_conversation')
  })

  it('rejects invalid command args before invoking Tauri', async () => {
    const invoke = vi.fn()
    const client = createInvokeCommandClient(invoke)

    await expect(startRun({ conversationId: '', prompt: '' }, client)).rejects.toThrow(
      TauriCommandPayloadError,
    )
    await expect(deleteConversation('', client)).rejects.toThrow(TauriCommandPayloadError)
    expect(invoke).not.toHaveBeenCalled()
  })

  it('supports explicit mock behavior for conversation commands', async () => {
    const client = createMockCommandClient()

    await expect(listConversations(client)).resolves.toHaveProperty('conversations')
    await expect(createConversation(client)).resolves.toHaveProperty('conversation.id')
    await expect(getConversation('conversation-001', client)).resolves.toHaveProperty(
      'conversation.id',
      'conversation-001',
    )
    await expect(
      startRun({ conversationId: 'conversation-001', prompt: 'Run' }, client),
    ).resolves.toHaveProperty('status', 'started')
    await expect(deleteConversation('conversation-001', client)).resolves.toHaveProperty(
      'status',
      'deleted',
    )
    await expect(cancelRun('run-001', client)).resolves.toHaveProperty('status', 'cancelled')
    await expect(
      resolvePermission(
        {
          conversationId: 'conversation-001',
          decision: 'deny',
          requestId: '01HZ0000000000000000000001',
        },
        client,
      ),
    ).resolves.toHaveProperty('decision', 'deny')
    await expect(
      listActivity({ conversationId: 'conversation-001' }, client),
    ).resolves.toHaveProperty('events')
    await expect(
      getContextSnapshot({ conversationId: 'conversation-001' }, client),
    ).resolves.toHaveProperty('project')
  })

  it('lists model provider catalog and saved provider profiles', async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === 'list_model_provider_catalog') {
        return {
          providers: [
            {
              defaultBaseUrl: 'https://api.openai.com',
              displayName: 'OpenAI',
              models: [
                {
                  protocol: 'responses',
                  conversationCapability: {
                    inputModalities: ['text'],
                    outputModalities: ['text'],
                    contextWindow: 128000,
                    maxOutputTokens: 16384,
                    streaming: true,
                    toolCalling: true,
                    reasoning: false,
                    promptCache: false,
                    structuredOutput: true,
                  },
                  contextWindow: 128000,
                  displayName: 'GPT-5.4 mini',
                  lifecycle: { kind: 'stable' },
                  maxOutputTokens: 16384,
                  modelId: 'gpt-5.4-mini',
                  runtimeStatus: { kind: 'runnable' },
                },
              ],
              providerId: 'openai',
              runtimeCapability: {
                authScheme: 'bearer',
                baseUrlRegions: [
                  { id: 'default', label: 'Default', baseUrl: 'https://api.openai.com' },
                ],
                supportsLiveValidation: true,
                supportsStreamingValidation: true,
                secretRevealSupported: true,
              },
              serviceCapabilities: [],
              sourceUrl: 'https://platform.openai.com/docs/models',
              verifiedDate: '2026-06-21',
            },
          ],
        }
      }

      return {
        defaultConfigId: 'openai',
        configs: [
          {
            protocol: 'responses',
            baseUrl: 'https://gateway.example.com',
            displayName: 'OpenAI gateway',
            hasApiKey: true,
            id: 'openai',
            isDefault: true,
            modelId: 'gpt-5.4-mini',
            modelDescriptor: {
              protocol: 'responses',
              conversationCapability: {
                inputModalities: ['text'],
                outputModalities: ['text'],
                contextWindow: 128000,
                maxOutputTokens: 16384,
                streaming: true,
                toolCalling: true,
                reasoning: false,
                promptCache: false,
                structuredOutput: true,
              },
              contextWindow: 128000,
              displayName: 'GPT-5.4 mini',
              lifecycle: { kind: 'stable' },
              maxOutputTokens: 16384,
              modelId: 'gpt-5.4-mini',
              runtimeStatus: { kind: 'runnable' },
            },
            providerId: 'openai',
          },
        ],
      }
    })
    const client = createInvokeCommandClient(invoke)

    await expect(listModelProviderCatalog(client)).resolves.toHaveProperty(
      'providers.0.defaultBaseUrl',
      'https://api.openai.com',
    )
    await expect(listProviderSettings(client)).resolves.toHaveProperty(
      'configs.0.baseUrl',
      'https://gateway.example.com',
    )
    expect(invoke).toHaveBeenCalledWith('list_model_provider_catalog')
    expect(invoke).toHaveBeenCalledWith('list_provider_settings')
  })

  it('rejects provider service categories outside the Rust contract', async () => {
    const invoke = vi.fn().mockResolvedValue({
      providers: [
        {
          defaultBaseUrl: 'https://api.minimaxi.com',
          displayName: 'MiniMax',
          models: [openAiModelDescriptor],
          providerId: 'minimax',
          runtimeCapability: {
            authScheme: 'bearer',
            baseUrlRegions: [{ id: 'cn', label: 'China', baseUrl: 'https://api.minimaxi.com' }],
            supportsLiveValidation: true,
            supportsStreamingValidation: true,
            secretRevealSupported: true,
          },
          serviceCapabilities: [
            {
              operationId: 'minimax.text_to_speech.sync',
              category: 'speech',
              inputModalities: ['text'],
              outputArtifact: 'audio',
              execution: 'sync',
              requiresPolling: false,
              permissionSubject: 'network:minimax',
              costRisk: 'medium',
            },
          ],
          sourceUrl: 'https://platform.minimax.io/docs/api-reference/text-chat-openai',
          verifiedDate: '2026-06-21',
        },
      ],
    })
    const client = createInvokeCommandClient(invoke)

    await expect(listModelProviderCatalog(client)).rejects.toThrow(TauriCommandPayloadError)
  })

  it('rejects provider configs without model descriptors', async () => {
    const invoke = vi.fn().mockResolvedValue({
      defaultConfigId: 'openai',
      configs: [
        {
          protocol: 'responses',
          displayName: 'OpenAI',
          hasApiKey: true,
          id: 'openai',
          isDefault: true,
          modelId: 'gpt-5.4-mini',
          providerId: 'openai',
        },
      ],
    })
    const client = createInvokeCommandClient(invoke)

    await expect(listProviderSettings(client)).rejects.toThrow(TauriCommandPayloadError)
  })

  it('validates and saves provider settings without returning raw keys', async () => {
    const providerToken = 'provider-test-token'
    const invoke = vi.fn(async (command: string) => {
      if (command === 'validate_provider_settings') {
        return {
          modelId: 'gpt-5.4-mini',
          providerId: 'openai',
          status: 'accepted',
        }
      }
      if (command === 'request_provider_config_api_key_reveal') {
        throw new Error('provider API key reveal is disabled')
      }
      if (command === 'get_provider_config_api_key') {
        throw new Error('provider API key reveal is disabled')
      }

      return {
        config: {
          protocol: 'responses',
          baseUrl: 'https://gateway.example.com',
          displayName: 'OpenAI gateway',
          hasApiKey: true,
          id: 'openai',
          isDefault: true,
          modelDescriptor: openAiModelDescriptor,
          modelId: 'gpt-5.4-mini',
          providerId: 'openai',
        },
        status: 'saved',
      }
    })
    const client = createInvokeCommandClient(invoke)

    await expect(
      validateProviderSettings(
        {
          modelId: 'gpt-5.4-mini',
          providerId: 'openai',
        },
        client,
      ),
    ).resolves.toEqual({
      modelId: 'gpt-5.4-mini',
      providerId: 'openai',
      status: 'accepted',
    })
    await expect(
      saveProviderSettings(
        {
          apiKey: providerToken,
          baseUrl: 'https://gateway.example.com',
          configId: 'openai',
          displayName: 'OpenAI gateway',
          modelId: 'gpt-5.4-mini',
          providerId: 'openai',
          setDefault: true,
        },
        client,
      ),
    ).resolves.toEqual({
      config: {
        protocol: 'responses',
        baseUrl: 'https://gateway.example.com',
        displayName: 'OpenAI gateway',
        hasApiKey: true,
        id: 'openai',
        isDefault: true,
        modelDescriptor: openAiModelDescriptor,
        modelId: 'gpt-5.4-mini',
        providerId: 'openai',
      },
      status: 'saved',
    })

    await expect(requestProviderConfigApiKeyReveal('openai', client)).rejects.toThrow('disabled')
    await expect(getProviderConfigApiKey('openai', 'reveal-token', client)).rejects.toThrow(
      'disabled',
    )

    expect(JSON.stringify(invoke.mock.results.slice(0, 2))).not.toContain(providerToken)
    expect(invoke).toHaveBeenCalledWith('validate_provider_settings', {
      modelId: 'gpt-5.4-mini',
      providerId: 'openai',
    })
    expect(invoke).toHaveBeenCalledWith('save_provider_settings', {
      apiKey: providerToken,
      baseUrl: 'https://gateway.example.com',
      configId: 'openai',
      displayName: 'OpenAI gateway',
      modelId: 'gpt-5.4-mini',
      providerId: 'openai',
      setDefault: true,
    })
    expect(invoke).toHaveBeenCalledWith('request_provider_config_api_key_reveal', {
      configId: 'openai',
    })
    expect(invoke).toHaveBeenCalledWith('get_provider_config_api_key', {
      configId: 'openai',
      revealToken: 'reveal-token',
    })
  })

  it('keeps mock provider API key reveal disabled by default', async () => {
    const client = createMockCommandClient()

    await expect(client.requestProviderConfigApiKeyReveal('openai')).rejects.toThrow('disabled')
    await expect(client.getProviderConfigApiKey('openai', 'reveal-token')).rejects.toThrow(
      'disabled',
    )
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

  it('models skill management commands through parsed payloads', async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === 'list_skills') {
        return {
          skills: [
            {
              description: 'Creates release notes from recent changes.',
              enabled: true,
              id: 'skill-001',
              importedAt: '2026-06-21T00:00:00.000Z',
              manageable: true,
              name: 'release-notes',
              sourceKind: 'workspace',
              status: 'ready',
              tags: ['writing'],
              updatedAt: '2026-06-21T00:00:00.000Z',
            },
          ],
        }
      }

      if (command === 'get_skill_detail') {
        return {
          skill: {
            bodyPreview: 'Write concise release notes.',
            configKeys: ['CHANGELOG_TOKEN'],
            files: [
              {
                kind: 'file',
                name: 'SKILL.md',
                path: 'SKILL.md',
                sizeBytes: 120,
                depth: 0,
              },
              {
                kind: 'directory',
                name: 'references',
                path: 'references',
                depth: 0,
              },
              {
                kind: 'file',
                name: 'style.md',
                path: 'references/style.md',
                sizeBytes: 80,
                depth: 1,
              },
            ],
            parameters: [
              {
                description: 'Target release version.',
                name: 'version',
                paramType: 'string',
                required: true,
              },
            ],
            summary: {
              description: 'Creates release notes from recent changes.',
              enabled: true,
              id: 'skill-001',
              manageable: true,
              name: 'release-notes',
              sourceKind: 'workspace',
              status: 'ready',
              tags: ['writing'],
            },
          },
        }
      }

      if (command === 'get_skill_file') {
        return {
          file: {
            content: 'Use terse release note bullets.',
            path: 'references/style.md',
          },
        }
      }

      if (command === 'import_skill') {
        return {
          skill: {
            description: 'Creates release notes from recent changes.',
            enabled: true,
            id: 'skill-001',
            importedAt: '2026-06-21T00:00:00.000Z',
            manageable: true,
            name: 'release-notes',
            sourceKind: 'workspace',
            status: 'ready',
            tags: ['writing'],
          },
        }
      }

      if (command === 'set_skill_enabled') {
        return {
          skill: {
            description: 'Creates release notes from recent changes.',
            enabled: false,
            id: 'skill-001',
            manageable: true,
            name: 'release-notes',
            sourceKind: 'workspace',
            status: 'disabled',
            tags: ['writing'],
          },
        }
      }

      return {
        id: 'skill-001',
        status: 'deleted',
      }
    })
    const client = createInvokeCommandClient(invoke)

    await expect(listSkills(client)).resolves.toHaveProperty('skills.0.name', 'release-notes')
    await expect(getSkillDetail('skill-001', client)).resolves.toMatchObject({
      skill: {
        bodyPreview: 'Write concise release notes.',
        files: [{ path: 'SKILL.md' }, { path: 'references' }, { path: 'references/style.md' }],
      },
    })
    await expect(getSkillFile('skill-001', 'references/style.md', client)).resolves.toMatchObject({
      file: {
        content: 'Use terse release note bullets.',
        path: 'references/style.md',
      },
    })
    await expect(importSkill('/tmp/release-notes', client)).resolves.toHaveProperty(
      'skill.sourceKind',
      'workspace',
    )
    await expect(setSkillEnabled('skill-001', false, client)).resolves.toHaveProperty(
      'skill.enabled',
      false,
    )
    await expect(deleteSkill('skill-001', client)).resolves.toEqual({
      id: 'skill-001',
      status: 'deleted',
    })

    expect(invoke).toHaveBeenCalledWith('list_skills')
    expect(invoke).toHaveBeenCalledWith('get_skill_detail', {
      id: 'skill-001',
    })
    expect(invoke).toHaveBeenCalledWith('get_skill_file', {
      id: 'skill-001',
      path: 'references/style.md',
    })
    expect(invoke).toHaveBeenCalledWith('import_skill', { sourcePath: '/tmp/release-notes' })
    expect(invoke).toHaveBeenCalledWith('set_skill_enabled', {
      enabled: false,
      id: 'skill-001',
    })
    expect(invoke).toHaveBeenCalledWith('delete_skill', { id: 'skill-001' })
  })

  it('rejects invalid skill command args and payloads', async () => {
    const invoke = vi.fn().mockResolvedValue({
      skills: [
        {
          description: '',
          enabled: true,
          id: 'skill-001',
          manageable: true,
          name: 'bad-skill',
          sourceKind: 'unknown',
          status: 'ready',
          tags: [],
        },
      ],
    })
    const client = createInvokeCommandClient(invoke)

    await expect(getSkillDetail('', client)).rejects.toThrow(TauriCommandPayloadError)
    await expect(getSkillFile('skill-001', '', client)).rejects.toThrow(TauriCommandPayloadError)
    await expect(importSkill('', client)).rejects.toThrow(TauriCommandPayloadError)
    await expect(setSkillEnabled('', true, client)).rejects.toThrow(TauriCommandPayloadError)
    await expect(deleteSkill('', client)).rejects.toThrow(TauriCommandPayloadError)
    await expect(listSkills(client)).rejects.toThrow(TauriCommandPayloadError)
  })
})
