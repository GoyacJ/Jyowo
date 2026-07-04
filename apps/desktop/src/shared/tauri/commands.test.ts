import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it, vi } from 'vitest'

function cursor(_label: string, conversationSequence = 1) {
  return { eventId: '01ARZ3NDEKTSV4RRFFQ69G5FAV', conversationSequence }
}

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T
}

const validEvidenceContentHash = 'a'.repeat(64)

function sharedWorktreeFixture() {
  return JSON.parse(
    readFileSync(
      resolve(
        process.cwd(),
        '../../crates/jyowo-harness-contracts/tests/fixtures/conversation_worktree_page.json',
      ),
      'utf8',
    ),
  ) as Record<string, unknown>
}

function validWorktreePage(): PageConversationWorktreeResponse {
  return {
    turns: [
      {
        id: 'turn:user-message-001',
        conversationId: 'conversation-001',
        position: 0,
        user: {
          id: 'user:user-message-001',
          messageId: 'user-message-001',
          body: 'Restore the product shell',
          clientMessageId: '00000000-0000-4000-8000-000000000001',
          timestamp: '2026-06-17T00:00:00.000Z',
          eventRefs: [{ eventId: 'event-001', cursor: cursor('', 1) }],
        },
        assistant: {
          id: 'assistant:run-001',
          runId: 'run-001',
          projectionVersion: 1,
          status: 'running',
          segments: [
            {
              kind: 'process',
              id: 'segment:process:run-001',
              order: 0,
              status: 'withheld',
              summary: '思考内容已折叠',
              steps: [
                {
                  id: 'process-step:run-001:reasoning',
                  order: 0,
                  kind: 'reasoning',
                  status: 'complete',
                  title: '推理过程',
                  body: 'Checked project context.',
                  eventRefs: [{ eventId: 'event-002', cursor: cursor('', 2) }],
                },
              ],
              eventRefs: [{ eventId: 'event-002', cursor: cursor('', 2) }],
            },
            {
              kind: 'text',
              id: 'segment:text:assistant-message-001',
              order: 1,
              messageId: 'assistant-message-001',
              body: 'I can help with that.',
              eventRefs: [{ eventId: 'event-003', cursor: cursor('', 3) }],
            },
            {
              kind: 'toolGroup',
              id: 'segment:tools:tool-use-001',
              order: 2,
              attempts: [
                {
                  id: 'tool:tool-use-001',
                  order: 0,
                  toolUseId: 'tool-use-001',
                  toolName: 'read_file',
                  status: 'failed',
                  permission: {
                    id: 'permission:request-001',
                    requestId: 'request-001',
                    toolUseId: 'tool-use-001',
                    status: 'approved',
                    operation: 'read',
                    target: { kind: 'file', label: 'test.ts' },
                    riskLevel: 'low',
                    reason: 'Approved once',
                    policy: { mode: 'default' },
                    decisionOptions: [
                      {
                        id: 'opt-1',
                        decision: 'approve',
                        label: 'Approve',
                        lifetime: 'once',
                        matcher: { kind: 'any', label: 'match all' },
                        requiresConfirmation: false,
                      },
                    ],
                    dataExposure: {
                      sendsWorkspaceData: false,
                      sendsNetworkData: false,
                      touchesPrivatePath: false,
                      secretRisk: 'none',
                    },
                    evidenceRefs: [{ eventId: 'event-004', cursor: cursor('', 4) }],
                  },
                  failureSummary: '工具执行失败。可在详情中查看。',
                  eventRefs: [{ eventId: 'event-005', cursor: cursor('', 5) }],
                },
              ],
              eventRefs: [{ eventId: 'event-006', cursor: cursor('', 6) }],
            },
            {
              kind: 'agentActivity',
              id: 'segment:agent:subagent-001',
              order: 3,
              activityKind: 'subagent',
              agentId: 'subagent-001',
              role: 'Reviewer',
              taskSummary: 'Review recent changes',
              status: 'completed',
              resultSummary: 'No blocking issues found.',
              eventRefs: [{ eventId: 'event-008', cursor: cursor('', 8) }],
            },
          ],
          eventRefs: [{ eventId: 'event-007', cursor: cursor('', 7) }],
        },
      },
    ],
    pageCursor: {
      turnId: 'turn:user-message-001',
      position: 0,
    },
    eventCursor: cursor('', 7),
    hasMoreBefore: false,
    hasMoreAfter: true,
    gap: false,
  }
}

const tauriListenSpy = vi.hoisted(() => vi.fn())

vi.mock('@tauri-apps/api/event', () => ({
  listen: tauriListenSpy,
}))

import { createTestCommandClient } from '@/testing/command-client'
import {
  archiveBackgroundAgent,
  cancelBackgroundAgent,
  cancelRun,
  clearMcpDiagnostics,
  createAttachmentFromPath,
  createConversation,
  createInvokeCommandClient,
  type DeleteAgentProfileRequest,
  deleteAgentProfile,
  deleteAutomation,
  deleteBackgroundAgent,
  deleteConversation,
  deleteMcpServer,
  deleteMemoryItem,
  deleteProject,
  deleteProviderCapabilityRoute,
  deleteSkill,
  exportMemoryItems,
  exportSupportBundle,
  getAppInfo,
  getArtifactMediaPreview,
  getAttachmentMediaPreview,
  getBackgroundAgent,
  getContextSnapshot,
  getConversation,
  getExecutionSettings,
  getHarnessHealthcheck,
  getMcpServerConfig,
  getMemoryItem,
  getModelUsageSummary,
  getPluginDetail,
  getProviderConfigApiKey,
  getReplayTimeline,
  getSkillCatalogEntry,
  getSkillCatalogFile,
  getSkillDetail,
  getSkillFile,
  importSkill,
  installPluginFromPath,
  installSkillFromCatalog,
  listActivity,
  listAgentProfiles,
  listArtifacts,
  listAutomationRuns,
  listAutomations,
  listBackgroundAgents,
  listBrowserMcpPresets,
  listConversations,
  listEvalCases,
  listenMcpDiagnosticBatches,
  listenSkillCatalogInstallProgress,
  listMcpDiagnostics,
  listMcpServers,
  listMemoryItems,
  listModelProviderCatalog,
  listOfficialQuotaSnapshots,
  listPlugins,
  listProviderCapabilityRouteOptions,
  listProviderCapabilityRoutes,
  listProviderProbeSnapshots,
  listProviderSettings,
  listReferenceCandidates,
  listSkillCatalogEntries,
  listSkillCatalogInstallTasks,
  listSkillCatalogSources,
  listSkills,
  type PageConversationWorktreeResponse,
  pageConversationWorktree,
  parseAgentCapabilities,
  parseAgentProfile,
  parseAgentToolPolicy,
  pauseBackgroundAgent,
  probeProviderConfig,
  refreshOfficialQuota,
  reloadPlugin,
  requestProviderConfigApiKeyReveal,
  resolvePermission,
  restartMcpServer,
  resumeBackgroundAgent,
  runAutomationNow,
  runEvalCase,
  type SaveAutomationRequest,
  type StartRunRequest,
  saveAgentProfile,
  saveAutomation,
  saveBrowserMcpPreset,
  saveMcpServer,
  saveProviderCapabilityRoute,
  saveProviderSettings,
  sendBackgroundAgentInput,
  setAutomationEnabled,
  setExecutionSettings,
  setMcpServerEnabled,
  setPluginEnabled,
  setProjectPluginsEnabled,
  setSkillEnabled,
  startRun,
  subscribeMcpDiagnostics,
  TauriCommandPayloadError,
  type ToolProfile,
  uninstallPlugin,
  unsubscribeMcpDiagnostics,
  updateMemoryItem,
  updatePluginConfig,
  validatePluginFromPath,
  validateProviderSettings,
} from './commands'
import { getCommandErrorMessage } from './errors'

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
const openAiRunModelSnapshot = {
  modelConfigId: 'provider-config-001',
  providerId: 'openai',
  modelId: openAiModelDescriptor.modelId,
  displayName: openAiModelDescriptor.displayName,
  protocol: openAiModelDescriptor.protocol,
} as const

describe('CommandClient', () => {
  const attachmentPreviewId =
    'attachment-1111111111111111111111111111111111111111111111111111111111111111'

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

  it('normalizes execution settings with context compression ratio', async () => {
    const agentCapabilities = {
      agentTeamsAvailable: false,
      agentTeamsEnabled: false,
      backgroundAgentsAvailable: false,
      backgroundAgentsEnabled: false,
      subagentsAvailable: false,
      subagentsEnabled: false,
      unavailableReasons: [],
    }
    const invoke = vi.fn().mockResolvedValue({
      agentCapabilities,
      autoModeAvailable: false,
      contextCompressionTriggerRatio: 0.8,
      permissionMode: 'default',
      toolProfile: 'coding',
    })
    const client = createInvokeCommandClient(invoke)

    await expect(getExecutionSettings(client)).resolves.toEqual({
      agentCapabilities,
      autoModeAvailable: false,
      contextCompressionTriggerRatio: 0.8,
      permissionMode: 'default',
      toolProfile: 'coding',
    })
    expect(invoke).toHaveBeenCalledWith('get_execution_settings')
  })

  it('validates execution settings save payload ratio bounds', async () => {
    const agentCapabilities = {
      agentTeamsAvailable: false,
      agentTeamsEnabled: false,
      backgroundAgentsAvailable: false,
      backgroundAgentsEnabled: false,
      subagentsAvailable: false,
      subagentsEnabled: false,
      unavailableReasons: [],
    }
    const invoke = vi.fn().mockResolvedValue({
      agentCapabilities,
      autoModeAvailable: false,
      contextCompressionTriggerRatio: 0.8,
      permissionMode: 'default',
      toolProfile: 'full',
    })
    const client = createInvokeCommandClient(invoke)

    await expect(
      setExecutionSettings(
        {
          agentTeamsEnabled: false,
          backgroundAgentsEnabled: false,
          contextCompressionTriggerRatio: 0.49,
          permissionMode: 'default',
          subagentsEnabled: false,
          toolProfile: 'full',
        },
        client,
      ),
    ).rejects.toBeInstanceOf(TauriCommandPayloadError)

    await setExecutionSettings(
      {
        agentTeamsEnabled: false,
        backgroundAgentsEnabled: false,
        contextCompressionTriggerRatio: 0.8,
        permissionMode: 'default',
        subagentsEnabled: false,
        toolProfile: {
          custom: {
            allowlist: ['read'],
            denylist: ['bash'],
            group_allowlist: ['file_system'],
            group_denylist: ['network'],
            mcp_included: false,
            plugin_included: false,
          },
        },
      },
      client,
    )
    expect(invoke).toHaveBeenCalledWith('set_execution_settings', {
      agentTeamsEnabled: false,
      backgroundAgentsEnabled: false,
      contextCompressionTriggerRatio: 0.8,
      permissionMode: 'default',
      subagentsEnabled: false,
      toolProfile: {
        custom: {
          allowlist: ['read'],
          denylist: ['bash'],
          group_allowlist: ['file_system'],
          group_denylist: ['network'],
          mcp_included: false,
          plugin_included: false,
        },
      },
    })
  })

  it('rejects unknown execution tool profiles before invoking IPC', async () => {
    const client = createInvokeCommandClient(vi.fn())

    await expect(
      setExecutionSettings(
        {
          agentTeamsEnabled: false,
          backgroundAgentsEnabled: false,
          contextCompressionTriggerRatio: 0.8,
          permissionMode: 'default',
          subagentsEnabled: false,
          toolProfile: 'unknown' as unknown as ToolProfile,
        },
        client,
      ),
    ).rejects.toBeInstanceOf(TauriCommandPayloadError)
  })

  it('models automation commands through strict IPC schemas', async () => {
    const automation = {
      id: 'checks',
      enabled: false,
      prompt: 'Run checks',
      schedule: { intervalMinutes: 30 },
      toolProfile: 'coding',
      permissionMode: 'default',
      sandboxMode: 'none',
      workspaceScope: 'current_workspace',
      workspaceAccess: 'read_only',
      missedRunPolicy: 'skip',
      createdAt: '2026-06-30T01:00:00Z',
      updatedAt: '2026-06-30T01:00:00Z',
    } as const
    const runRecord = {
      automationId: 'checks',
      completedAt: '2026-06-30T01:01:00Z',
      id: 'automation-run-001',
      message: 'Starting automation runs requires the runtime conversation facade.',
      runId: undefined,
      startedAt: '2026-06-30T01:00:00Z',
      status: 'rejected',
    } as const
    const invoke = vi.fn(async (command: string) => {
      if (command === 'list_automations') {
        return { automations: [automation] }
      }
      if (command === 'save_automation') {
        return { automation, status: 'saved' }
      }
      if (command === 'set_automation_enabled') {
        return {
          automation: { ...automation, enabled: true },
          status: 'saved',
        }
      }
      if (command === 'run_automation_now') {
        return { record: runRecord }
      }
      if (command === 'list_automation_runs') {
        return { runs: [runRecord] }
      }
      if (command === 'delete_automation') {
        return { id: 'checks', status: 'deleted' }
      }
      throw new Error(`unexpected command: ${command}`)
    })
    const client = createInvokeCommandClient(invoke)

    await expect(listAutomations(client)).resolves.toEqual({
      automations: [automation],
    })
    await expect(saveAutomation({ automation }, client)).resolves.toEqual({
      automation,
      status: 'saved',
    })
    await expect(setAutomationEnabled('checks', true, client)).resolves.toEqual({
      automation: { ...automation, enabled: true },
      status: 'saved',
    })
    await expect(runAutomationNow('checks', client)).resolves.toEqual({
      record: runRecord,
    })
    await expect(listAutomationRuns('checks', client)).resolves.toEqual({
      runs: [runRecord],
    })
    await expect(deleteAutomation('checks', client)).resolves.toEqual({
      id: 'checks',
      status: 'deleted',
    })

    expect(invoke).toHaveBeenCalledWith('list_automations')
    expect(invoke).toHaveBeenCalledWith('save_automation', { automation })
    expect(invoke).toHaveBeenCalledWith('set_automation_enabled', {
      enabled: true,
      id: 'checks',
    })
    expect(invoke).toHaveBeenCalledWith('run_automation_now', { id: 'checks' })
    expect(invoke).toHaveBeenCalledWith('list_automation_runs', {
      automationId: 'checks',
    })
    expect(invoke).toHaveBeenCalledWith('delete_automation', { id: 'checks' })
    expect(JSON.stringify(runRecord)).not.toContain('rawToolOutput')
  })

  it('rejects unsupported automation snapshots before invoking IPC', async () => {
    const client = createInvokeCommandClient(vi.fn())
    const automation = {
      id: 'checks',
      enabled: false,
      prompt: 'Run checks',
      schedule: { intervalMinutes: 30 },
      toolProfile: 'unknown',
      permissionMode: 'default',
      sandboxMode: 'none',
      workspaceScope: 'current_workspace',
      workspaceAccess: 'read_only',
      missedRunPolicy: 'skip',
      createdAt: '2026-06-30T01:00:00Z',
      updatedAt: '2026-06-30T01:00:00Z',
    } as unknown as SaveAutomationRequest['automation']

    await expect(saveAutomation({ automation }, client)).rejects.toBeInstanceOf(
      TauriCommandPayloadError,
    )
    await expect(
      saveAutomation(
        {
          automation: {
            ...automation,
            toolProfile: 'coding',
            missedRunPolicy: 'catch_up_all',
          } as unknown as SaveAutomationRequest['automation'],
        },
        client,
      ),
    ).rejects.toBeInstanceOf(TauriCommandPayloadError)
  })

  it('rejects secret-like automation prompts before invoking IPC', async () => {
    const invoke = vi.fn()
    const client = createInvokeCommandClient(invoke)
    const automation = {
      id: 'checks',
      enabled: false,
      prompt: 'Use token=automation-secret-123456',
      schedule: { intervalMinutes: 30 },
      toolProfile: 'coding',
      permissionMode: 'default',
      sandboxMode: 'none',
      workspaceScope: 'current_workspace',
      workspaceAccess: 'read_only',
      missedRunPolicy: 'skip',
      createdAt: '2026-06-30T01:00:00Z',
      updatedAt: '2026-06-30T01:00:00Z',
    } as const

    await expect(saveAutomation({ automation }, client)).rejects.toBeInstanceOf(
      TauriCommandPayloadError,
    )
    expect(invoke).not.toHaveBeenCalled()
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

  it('supports test clients outside the Tauri runtime', async () => {
    const client = createTestCommandClient()

    await expect(getAppInfo(client)).resolves.toMatchObject({
      name: 'Jyowo',
      shell: 'tauri2-react',
    })
    await expect(getHarnessHealthcheck(client)).resolves.toEqual({
      status: 'available',
      sdkCrate: 'jyowo_harness_sdk',
    })
  })

  it('keeps fixture timeline subscription replay separate from activity defaults', async () => {
    const defaultClient = createTestCommandClient()

    await expect(
      defaultClient.subscribeConversationEvents({
        conversationId: 'conversation-001',
      }),
    ).resolves.toMatchObject({
      conversationId: 'conversation-001',
      replayEvents: [],
      gap: false,
    })

    const streamingClient = createTestCommandClient({
      subscribeConversationEvents: {
        subscriptionId: 'subscription-stream',
        conversationId: 'conversation-001',
        replayEvents: [
          {
            id: 'evt-delta',
            conversationSequence: 1,
            payload: { messageId: 'message-streamed', text: 'streamed' },
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
      streamingClient.subscribeConversationEvents({
        conversationId: 'conversation-001',
      }),
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
    expect(invoke).toHaveBeenCalledWith('get_conversation', {
      conversationId: 'conversation-001',
    })
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

  it('models project deletion through Zod validation', async () => {
    const invoke = vi.fn().mockResolvedValue({
      activePath: null,
      path: '/Users/goya/Repo/Git/Jyowo',
      status: 'deleted',
    })
    const client = createInvokeCommandClient(invoke)

    await expect(deleteProject('/Users/goya/Repo/Git/Jyowo', client)).resolves.toEqual({
      activePath: null,
      path: '/Users/goya/Repo/Git/Jyowo',
      status: 'deleted',
    })
    expect(invoke).toHaveBeenCalledWith('delete_project', {
      path: '/Users/goya/Repo/Git/Jyowo',
    })
  })

  it('models conversation worktree pages through Zod validation', async () => {
    const invoke = vi.fn().mockResolvedValue(validWorktreePage())
    const client = createInvokeCommandClient(invoke)

    await expect(
      pageConversationWorktree(
        {
          conversationId: 'conversation-001',
          pageCursor: { turnId: 'turn:user-message-000', position: 0 },
          direction: 'after',
          limit: 20,
        },
        client,
      ),
    ).resolves.toMatchObject({
      turns: [
        {
          id: 'turn:user-message-001',
          position: 0,
          user: { messageId: 'user-message-001' },
          assistant: {
            id: 'assistant:run-001',
            segments: [
              {
                kind: 'process',
                id: 'segment:process:run-001',
                steps: [
                  {
                    kind: 'reasoning',
                    body: 'Checked project context.',
                  },
                ],
              },
              { kind: 'text', id: 'segment:text:assistant-message-001' },
              {
                kind: 'toolGroup',
                attempts: [
                  {
                    toolUseId: 'tool-use-001',
                    permission: {
                      requestId: 'request-001',
                      toolUseId: 'tool-use-001',
                    },
                  },
                ],
              },
              {
                kind: 'agentActivity',
                id: 'segment:agent:subagent-001',
                activityKind: 'subagent',
                agentId: 'subagent-001',
                status: 'completed',
              },
            ],
          },
        },
      ],
      pageCursor: { turnId: 'turn:user-message-001', position: 0 },
      eventCursor: cursor('', 7),
      hasMoreBefore: false,
      hasMoreAfter: true,
      gap: false,
    })
    expect(invoke).toHaveBeenCalledWith('page_conversation_worktree', {
      conversationId: 'conversation-001',
      pageCursor: { turnId: 'turn:user-message-000', position: 0 },
      direction: 'after',
      limit: 20,
    })
  })

  it('accepts ordinary token-counting text in conversation worktree pages', async () => {
    const payload = clone(validWorktreePage())
    const assistant = payload.turns[0].assistant
    if (!assistant) {
      throw new Error('assistant fixture missing')
    }
    assistant.segments.push({
      kind: 'text',
      id: 'segment:text:token-counting',
      order: 3,
      messageId: 'assistant-message-token-counting',
      body: 'Anthropic compatible API, model list, Token 计数, Token: 统计, and token authentication.',
    } as (typeof assistant.segments)[number])

    await expect(
      pageConversationWorktree(
        { conversationId: 'conversation-001' },
        createInvokeCommandClient(vi.fn().mockResolvedValue(payload)),
      ),
    ).resolves.toMatchObject({
      turns: [
        {
          assistant: {
            segments: expect.arrayContaining([
              expect.objectContaining({
                body: 'Anthropic compatible API, model list, Token 计数, Token: 统计, and token authentication.',
              }),
            ]),
          },
        },
      ],
    })
  })

  it('models process segments and artifact media through Zod validation', async () => {
    const payload = validWorktreePage()
    const assistant = payload.turns[0].assistant
    if (!assistant) {
      throw new Error('assistant fixture missing')
    }
    assistant.segments = [
      {
        kind: 'process',
        id: 'segment:process:run-001',
        order: 0,
        status: 'complete',
        summary: '已完成工作过程',
        steps: [
          {
            id: 'process-step:reasoning',
            order: 0,
            kind: 'reasoning',
            status: 'complete',
            title: '分析请求',
            body: '确认需要生成图片并展示结果。',
            eventRefs: [{ eventId: 'event-process-001', cursor: cursor('', 2) }],
          },
          {
            id: 'process-step:command',
            order: 1,
            kind: 'command',
            status: 'complete',
            title: '运行检查',
            detail: {
              type: 'command',
              command: 'pnpm check:desktop',
              stdoutPreview: 'passed',
              exitCode: 0,
              durationMs: 1234,
              truncated: false,
              redactionState: 'clean',
              riskLevel: 'low',
            },
          },
          {
            id: 'process-step:artifact',
            order: 2,
            kind: 'artifact',
            status: 'complete',
            title: '生成的图片',
            detail: {
              type: 'artifact',
              artifactId: 'artifact-image-001',
              media: {
                kind: 'image',
                mimeType: 'image/png',
                sizeBytes: 128,
              },
            },
          },
        ],
        eventRefs: [{ eventId: 'event-process-001', cursor: cursor('', 2) }],
      },
      {
        kind: 'artifact',
        id: 'segment:artifact:artifact-image-001',
        order: 1,
        artifactId: 'artifact-image-001',
        artifactKind: 'image',
        status: 'ready',
        source: 'tool',
        title: 'Generated image',
        summary: 'Image artifact ready',
        revision: {
          artifactId: 'artifact-image-001',
          revisionId: 'rev-img-1',
          kind: 'image',
          status: 'ready',
          sourceRunId: 'run-001',
          title: 'Generated image',
          media: {
            kind: 'image',
            mimeType: 'image/png',
            sizeBytes: 128,
          },
        },
      },
      {
        kind: 'text',
        id: 'segment:text:assistant-message-001',
        order: 2,
        messageId: 'assistant-message-001',
        body: '图片已生成。',
      },
    ]

    await expect(
      pageConversationWorktree(
        { conversationId: 'conversation-001' },
        createInvokeCommandClient(vi.fn().mockResolvedValue(payload)),
      ),
    ).resolves.toMatchObject({
      turns: [
        {
          assistant: {
            segments: [
              {
                kind: 'process',
                status: 'complete',
                steps: [
                  { kind: 'reasoning', body: '确认需要生成图片并展示结果。' },
                  {
                    kind: 'command',
                    detail: {
                      type: 'command',
                      command: 'pnpm check:desktop',
                      exitCode: 0,
                    },
                  },
                  {
                    kind: 'artifact',
                    detail: {
                      type: 'artifact',
                      artifactId: 'artifact-image-001',
                      media: {
                        kind: 'image',
                        mimeType: 'image/png',
                        sizeBytes: 128,
                      },
                    },
                  },
                ],
              },
              {
                kind: 'artifact',
                artifactKind: 'image',
                status: 'ready',
                source: 'tool',
                revision: { media: { kind: 'image', mimeType: 'image/png', sizeBytes: 128 } },
              },
              { kind: 'text', body: '图片已生成。' },
            ],
          },
        },
      ],
    })
  })

  it('validates the shared Rust conversation worktree page fixture', async () => {
    const fixture = sharedWorktreeFixture()
    await expect(
      pageConversationWorktree(
        { conversationId: 'conversation-001' },
        createInvokeCommandClient(vi.fn().mockResolvedValue(fixture)),
      ),
    ).resolves.toMatchObject({
      turns: [
        {
          assistant: {
            segments: [
              { kind: 'process' },
              { kind: 'text' },
              { kind: 'toolGroup' },
              {
                kind: 'artifact',
                artifactKind: 'image',
                status: 'ready',
                source: 'tool',
                revision: { media: { kind: 'image', mimeType: 'image/png', sizeBytes: 128 } },
              },
              { kind: 'reviewRequest' },
              { kind: 'clarificationRequest' },
              { kind: 'notice' },
              { kind: 'error' },
            ],
          },
        },
      ],
      hasMoreBefore: true,
      hasMoreAfter: true,
      gap: false,
    })
  })

  it('accepts structured notice codes in conversation worktree pages', async () => {
    const payload = validWorktreePage()
    const assistant = payload.turns[0].assistant
    if (!assistant) {
      throw new Error('assistant fixture missing')
    }
    assistant.segments = [
      {
        kind: 'notice',
        id: 'segment:notice:context-compacted',
        order: 0,
        body: '上下文已自动压缩',
        code: 'contextCompacted',
      },
      {
        kind: 'notice',
        id: 'segment:notice:future',
        order: 1,
        body: 'Future notice',
        code: 'futureNoticeCode',
      },
    ]

    await expect(
      pageConversationWorktree(
        { conversationId: 'conversation-001' },
        createInvokeCommandClient(vi.fn().mockResolvedValue(payload)),
      ),
    ).resolves.toMatchObject({
      turns: [
        {
          assistant: {
            segments: [
              { kind: 'notice', code: 'contextCompacted' },
              { kind: 'notice', code: 'futureNoticeCode' },
            ],
          },
        },
      ],
    })
  })

  it('accepts historical user attachments in conversation worktree pages', async () => {
    const payload = validWorktreePage() as unknown as Record<string, unknown>
    const turns = payload.turns as Array<Record<string, unknown>>
    const user = turns[0].user as Record<string, unknown>
    user.attachments = [
      {
        blob_ref: {
          content_hash: validBlobRef.contentHash,
          content_type: validBlobRef.contentType,
          id: validBlobRef.id,
          size: validBlobRef.size,
        },
        id: validAttachmentId,
        mime_type: 'text/plain',
        name: 'notes.txt',
        size_bytes: 128,
      },
    ]

    await expect(
      pageConversationWorktree(
        { conversationId: 'conversation-001' },
        createInvokeCommandClient(vi.fn().mockResolvedValue(payload)),
      ),
    ).resolves.toMatchObject({
      turns: [
        {
          user: {
            attachments: [
              {
                id: validAttachmentId,
                mimeType: 'text/plain',
                name: 'notes.txt',
                sizeBytes: 128,
              },
            ],
          },
        },
      ],
    })
  })

  it('rejects unsafe historical attachment metadata in conversation worktree pages', async () => {
    const payload = validWorktreePage() as unknown as Record<string, unknown>
    const turns = payload.turns as Array<Record<string, unknown>>
    const user = turns[0].user as Record<string, unknown>
    user.attachments = [
      {
        blobRef: {
          ...validBlobRef,
          contentType: 'file:///Users/alice/private.txt',
        },
        id: validAttachmentId,
        mimeType: 'text/plain authorization bearer secret-token',
        name: '/Users/alice/.ssh/id_rsa',
        sizeBytes: 128,
      },
    ]

    await expect(
      pageConversationWorktree(
        { conversationId: 'conversation-001' },
        createInvokeCommandClient(vi.fn().mockResolvedValue(payload)),
      ),
    ).rejects.toThrow()
  })

  it('rejects malformed conversation worktree payloads at the IPC boundary', async () => {
    const invalidCases = [
      (page: ReturnType<typeof validWorktreePage>) => {
        delete (page.turns[0] as Partial<(typeof page.turns)[number]>).position
      },
      (page: ReturnType<typeof validWorktreePage>) => {
        const assistant = page.turns[0].assistant
        if (!assistant) {
          throw new Error('assistant fixture missing')
        }
        delete (assistant.segments[0] as { id?: string }).id
      },
      (page: ReturnType<typeof validWorktreePage>) => {
        delete (page.turns[0] as Partial<(typeof page.turns)[number]>).user
      },
      (page: ReturnType<typeof validWorktreePage>) => {
        const assistant = page.turns[0].assistant
        if (!assistant) {
          throw new Error('assistant fixture missing')
        }
        const segment = assistant.segments[2]
        if (segment.kind === 'toolGroup') {
          const [attempt] = segment.attempts ?? []
          if (!attempt) {
            throw new Error('tool attempt fixture missing')
          }
          delete (attempt as { toolUseId?: string }).toolUseId
        }
      },
      (page: ReturnType<typeof validWorktreePage>) => {
        const assistant = page.turns[0].assistant
        if (!assistant) {
          throw new Error('assistant fixture missing')
        }
        const segment = assistant.segments[2]
        if (segment.kind === 'toolGroup') {
          const [attempt] = segment.attempts ?? []
          if (!attempt) {
            throw new Error('tool attempt fixture missing')
          }
          delete (attempt.permission as { requestId?: string }).requestId
        }
      },
      (page: ReturnType<typeof validWorktreePage>) => {
        page.turns[0].user.body = '/Users/goya/.ssh/config'
      },
      (page: ReturnType<typeof validWorktreePage>) => {
        const assistant = page.turns[0].assistant
        if (!assistant) {
          throw new Error('assistant fixture missing')
        }
        Object.assign(assistant.segments[0], { text: 'raw private chain' })
      },
    ]

    for (const mutate of invalidCases) {
      const payload = clone(validWorktreePage())
      mutate(payload)

      await expect(
        pageConversationWorktree(
          { conversationId: 'conversation-001' },
          createInvokeCommandClient(vi.fn().mockResolvedValue(payload)),
        ),
      ).rejects.toThrow(TauriCommandPayloadError)
    }
  })

  it('rejects oversized worktree diff previews at the IPC boundary', async () => {
    const payload = clone(validWorktreePage())
    const assistant = payload.turns[0].assistant
    if (!assistant) {
      throw new Error('assistant fixture missing')
    }
    assistant.segments.push({
      kind: 'process',
      id: 'segment:process:oversized-diff-preview',
      order: 10,
      status: 'complete',
      summary: 'Oversized diff preview',
      steps: [
        {
          id: 'process-step:oversized-diff-preview',
          order: 0,
          kind: 'diff',
          status: 'complete',
          title: 'Review diff',
          detail: {
            type: 'diff',
            id: 'change-set-oversized-preview',
            summary: 'Oversized preview',
            files: [
              {
                path: 'src/oversized.ts',
                status: 'modified',
                addedLines: 1,
                removedLines: 1,
                preview: 'x'.repeat(70_001),
              },
            ],
          },
        },
      ],
      eventRefs: [{ eventId: 'event-diff-oversized', cursor: cursor('', 8) }],
    })

    await expect(
      pageConversationWorktree(
        { conversationId: 'conversation-001' },
        createInvokeCommandClient(vi.fn().mockResolvedValue(payload)),
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
  })

  it('rejects oversized worktree command previews at the IPC boundary', async () => {
    const payload = clone(validWorktreePage())
    const assistant = payload.turns[0].assistant
    if (!assistant) {
      throw new Error('assistant fixture missing')
    }
    assistant.segments.push({
      kind: 'process',
      id: 'segment:process:oversized-command-preview',
      order: 10,
      status: 'complete',
      summary: 'Oversized command preview',
      steps: [
        {
          id: 'process-step:oversized-command-preview',
          order: 0,
          kind: 'command',
          status: 'complete',
          title: 'Run command',
          detail: {
            type: 'command',
            command: 'pnpm check',
            stdoutPreview: 'x'.repeat(70_001),
            truncated: true,
            redactionState: 'clean',
            riskLevel: 'low',
          },
        },
      ],
      eventRefs: [{ eventId: 'event-command-oversized', cursor: cursor('', 8) }],
    })

    await expect(
      pageConversationWorktree(
        { conversationId: 'conversation-001' },
        createInvokeCommandClient(vi.fn().mockResolvedValue(payload)),
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
  })

  it('rejects oversized evidence content at the IPC boundary', async () => {
    const client = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        byteLength: 70_001,
        contentHash: validEvidenceContentHash,
        contentBytes: 70_001,
        contentType: 'text/plain; charset=utf-8',
        hasMore: false,
        hashAlgorithm: 'blake3',
        kind: 'command-output',
        limitBytes: 65_536,
        maxBytes: 65_536,
        offsetBytes: 0,
        output: 'x'.repeat(70_001),
        redactionState: 'clean',
        refId: 'evidence-command-output-001',
        returnedBytes: 70_001,
        totalBytes: 70_001,
        truncated: false,
      }),
    )

    await expect(
      client.getConversationCommandOutput({
        conversationId: 'conversation-001',
        fullOutputRef: 'evidence-command-output-001',
      }),
    ).rejects.toThrow(TauriCommandPayloadError)
  })

  it('rejects malformed evidence content hashes at the IPC boundary', async () => {
    const client = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        byteLength: 7,
        contentHash: 'hash-command-output',
        contentBytes: 7,
        contentType: 'text/plain; charset=utf-8',
        hasMore: false,
        hashAlgorithm: 'blake3',
        kind: 'command-output',
        limitBytes: 65_536,
        maxBytes: 65_536,
        offsetBytes: 0,
        output: 'bounded',
        redactionState: 'clean',
        refId: 'evidence-command-output-001',
        returnedBytes: 7,
        totalBytes: 7,
        truncated: false,
      }),
    )

    await expect(
      client.getConversationCommandOutput({
        conversationId: 'conversation-001',
        fullOutputRef: 'evidence-command-output-001',
      }),
    ).rejects.toThrow(TauriCommandPayloadError)
  })

  it('rejects unsupported evidence hash algorithms at the IPC boundary', async () => {
    const client = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        byteLength: 7,
        contentHash: validEvidenceContentHash,
        contentBytes: 7,
        contentType: 'text/plain; charset=utf-8',
        hasMore: false,
        hashAlgorithm: 'sha256',
        kind: 'command-output',
        limitBytes: 65_536,
        maxBytes: 65_536,
        offsetBytes: 0,
        output: 'bounded',
        redactionState: 'clean',
        refId: 'evidence-command-output-001',
        returnedBytes: 7,
        totalBytes: 7,
        truncated: false,
      }),
    )

    await expect(
      client.getConversationCommandOutput({
        conversationId: 'conversation-001',
        fullOutputRef: 'evidence-command-output-001',
      }),
    ).rejects.toThrow(TauriCommandPayloadError)
  })

  it('rejects oversized diff patch content at the IPC boundary', async () => {
    const client = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        byteLength: 70_001,
        contentHash: validEvidenceContentHash,
        contentBytes: 70_001,
        contentType: 'text/x-diff; charset=utf-8',
        hasMore: false,
        hashAlgorithm: 'blake3',
        kind: 'diff-patch',
        limitBytes: 65_536,
        maxBytes: 65_536,
        offsetBytes: 0,
        patch: 'x'.repeat(70_001),
        redactionState: 'clean',
        refId: 'evidence-diff-patch-001',
        returnedBytes: 70_001,
        totalBytes: 70_001,
        truncated: false,
      }),
    )

    await expect(
      client.getConversationDiffPatch({
        conversationId: 'conversation-001',
        fullPatchRef: 'evidence-diff-patch-001',
      }),
    ).rejects.toThrow(TauriCommandPayloadError)
  })

  it('rejects oversized artifact content at the IPC boundary', async () => {
    const client = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        artifactId: 'artifact-001',
        byteLength: 70_001,
        content: 'x'.repeat(70_001),
        contentHash: validEvidenceContentHash,
        contentBytes: 70_001,
        contentType: 'text/markdown; charset=utf-8',
        hasMore: false,
        hashAlgorithm: 'blake3',
        kind: 'artifact-content',
        limitBytes: 65_536,
        maxBytes: 65_536,
        offsetBytes: 0,
        redactionState: 'clean',
        refId: 'evidence-artifact-content-001',
        returnedBytes: 70_001,
        totalBytes: 70_001,
        truncated: false,
      }),
    )

    await expect(
      client.getArtifactRevisionContent({
        contentRef: 'evidence-artifact-content-001',
        conversationId: 'conversation-001',
      }),
    ).rejects.toThrow(TauriCommandPayloadError)
  })

  it('exports evidence refs without returning evidence content through IPC', async () => {
    const invoke = vi.fn().mockResolvedValue({
      byteLength: 131_072,
      contentType: 'text/plain; charset=utf-8',
      exportedAt: '2026-06-17T02:22:00.000Z',
      kind: 'command-output',
      path: '.jyowo/runtime/exports/evidence-command-output-20260617T022200.000Z.txt',
      refId: 'evidence-command-output-001',
    })
    const client = createInvokeCommandClient(invoke)

    const response = await client.exportConversationEvidence({
      conversationId: 'conversation-001',
      kind: 'command-output',
      refId: 'evidence-command-output-001',
    })

    expect(response).toEqual({
      byteLength: 131_072,
      contentType: 'text/plain; charset=utf-8',
      exportedAt: '2026-06-17T02:22:00.000Z',
      kind: 'command-output',
      path: '.jyowo/runtime/exports/evidence-command-output-20260617T022200.000Z.txt',
      refId: 'evidence-command-output-001',
    })
    expect(invoke).toHaveBeenCalledWith('export_conversation_evidence', {
      conversationId: 'conversation-001',
      kind: 'command-output',
      refId: 'evidence-command-output-001',
    })
  })

  it('rejects unsafe image media metadata in conversation worktree pages', async () => {
    const payload = clone(validWorktreePage())
    const assistant = payload.turns[0].assistant
    if (!assistant) {
      throw new Error('assistant fixture missing')
    }
    assistant.segments.push({
      kind: 'artifact',
      id: 'segment:artifact:svg',
      order: 3,
      artifactId: 'artifact-svg',
      artifactKind: 'image',
      status: 'ready',
      source: 'tool',
      title: 'Generated SVG',
      media: {
        kind: 'image',
        mimeType: 'image/svg+xml',
        sizeBytes: 12,
      },
    } as unknown as (typeof assistant.segments)[number])

    await expect(
      pageConversationWorktree(
        { conversationId: 'conversation-001' },
        createInvokeCommandClient(vi.fn().mockResolvedValue(payload)),
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
  })

  it('rejects unsafe non-image media metadata in conversation worktree pages', async () => {
    const payload = clone(validWorktreePage())
    const assistant = payload.turns[0].assistant
    if (!assistant) {
      throw new Error('assistant fixture missing')
    }
    assistant.segments.push({
      kind: 'artifact',
      id: 'segment:artifact:video',
      order: 3,
      artifactId: 'artifact-video',
      artifactKind: 'video',
      status: 'ready',
      source: 'tool',
      title: 'Generated video',
      media: {
        kind: 'video',
        mimeType: 'video/mp4 https://provider.example/video',
        sizeBytes: 12,
      },
    } as unknown as (typeof assistant.segments)[number])

    await expect(
      pageConversationWorktree(
        { conversationId: 'conversation-001' },
        createInvokeCommandClient(vi.fn().mockResolvedValue(payload)),
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
  })

  it('rejects secret-like artifact media MIME tokens in conversation worktree pages', async () => {
    const payload = clone(validWorktreePage())
    const assistant = payload.turns[0].assistant
    if (!assistant) {
      throw new Error('assistant fixture missing')
    }
    assistant.segments.push({
      kind: 'artifact',
      id: 'segment:artifact:video',
      order: 3,
      artifactId: 'artifact-video',
      artifactKind: 'video',
      status: 'ready',
      source: 'tool',
      title: 'Generated video',
      media: {
        kind: 'video',
        mimeType: 'video/sk-abcdefghijklmnopqrstuvwxyz0123456789',
        sizeBytes: 12,
      },
    } as unknown as (typeof assistant.segments)[number])

    await expect(
      pageConversationWorktree(
        { conversationId: 'conversation-001' },
        createInvokeCommandClient(vi.fn().mockResolvedValue(payload)),
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
  })

  it.each([
    'https://provider.example/artifact?signature=abc',
    'data:image/svg+xml;base64,PHN2Zy8+',
    'blob:.jyowo/runtime/blobs/blob-001',
    'file:///Users/goya/private/image.png',
    'javascript:alert(1)',
    'mailto:user@example.com',
    'token=secret',
    '.jyowo/runtime/blobs/blob-001',
    '.JYOWO/runtime/blobs/blob-001',
    '/tmp/provider-output',
    'C:\\Users\\goya\\private\\image.png',
  ])('rejects unsafe conversation display text in worktree pages: %s', async (unsafeText) => {
    const payload = clone(validWorktreePage())
    const assistant = payload.turns[0].assistant
    if (!assistant) {
      throw new Error('assistant fixture missing')
    }
    assistant.segments.push({
      kind: 'artifact',
      id: 'segment:artifact:unsafe-title',
      order: 3,
      artifactId: 'artifact-unsafe-title',
      artifactKind: 'file',
      status: 'ready',
      source: 'tool',
      title: `Unsafe ${unsafeText}`,
    } as (typeof assistant.segments)[number])

    await expect(
      pageConversationWorktree(
        { conversationId: 'conversation-001' },
        createInvokeCommandClient(vi.fn().mockResolvedValue(payload)),
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
  })

  it('accepts allowlisted file media metadata in conversation worktree pages', async () => {
    const payload = clone(validWorktreePage())
    const assistant = payload.turns[0].assistant
    if (!assistant) {
      throw new Error('assistant fixture missing')
    }
    assistant.segments.push({
      kind: 'artifact',
      id: 'segment:artifact:file',
      order: 3,
      artifactId: 'artifact-file',
      artifactKind: 'file',
      status: 'ready',
      source: 'tool',
      title: 'Generated file',
      revision: {
        artifactId: 'artifact-file',
        revisionId: 'rev-file-1',
        kind: 'file',
        status: 'ready',
        sourceRunId: 'run-001',
        title: 'Generated file',
        media: {
          kind: 'file',
          mimeType: 'text/plain',
          sizeBytes: 12,
        },
      },
    } as (typeof assistant.segments)[number])

    await expect(
      pageConversationWorktree(
        { conversationId: 'conversation-001' },
        createInvokeCommandClient(vi.fn().mockResolvedValue(payload)),
      ),
    ).resolves.toEqual(payload)
  })

  it.each([
    ['video', 'audio/mpeg'],
    ['audio', 'video/mp4'],
    ['file', 'image/png'],
  ] as const)('rejects %s artifact media with %s MIME type', async (kind, mimeType) => {
    const payload = clone(validWorktreePage())
    const assistant = payload.turns[0].assistant
    if (!assistant) {
      throw new Error('assistant fixture missing')
    }
    assistant.segments.push({
      kind: 'artifact',
      id: `segment:artifact:${kind}-mismatch`,
      order: 3,
      artifactId: `artifact-${kind}-mismatch`,
      artifactKind: kind,
      status: 'ready',
      source: 'tool',
      title: 'Generated artifact',
      media: {
        kind,
        mimeType,
        sizeBytes: 12,
      },
    } as unknown as (typeof assistant.segments)[number])

    await expect(
      pageConversationWorktree(
        { conversationId: 'conversation-001' },
        createInvokeCommandClient(vi.fn().mockResolvedValue(payload)),
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
  })

  it('rejects invalid agent activity segment payloads', async () => {
    const invalidCases = [
      (page: ReturnType<typeof validWorktreePage>) => {
        const assistant = page.turns[0].assistant
        if (!assistant) {
          throw new Error('assistant fixture missing')
        }
        assistant.segments.push({
          kind: 'agentActivity',
          id: 'segment:agent:invalid',
          order: 4,
          activityKind: 'unknown-kind',
          agentId: 'subagent-002',
          role: 'Reviewer',
          taskSummary: 'Review recent changes',
          status: 'running',
        } as unknown as (typeof assistant.segments)[number])
      },
      (page: ReturnType<typeof validWorktreePage>) => {
        const assistant = page.turns[0].assistant
        if (!assistant) {
          throw new Error('assistant fixture missing')
        }
        assistant.segments.push({
          kind: 'agentActivity',
          id: 'segment:agent:invalid-status',
          order: 4,
          activityKind: 'subagent',
          agentId: 'subagent-002',
          role: 'Reviewer',
          taskSummary: 'Review recent changes',
          status: 'unknown-status',
        } as unknown as (typeof assistant.segments)[number])
      },
    ]

    for (const mutate of invalidCases) {
      const payload = clone(validWorktreePage())
      mutate(payload)

      await expect(
        pageConversationWorktree(
          { conversationId: 'conversation-001' },
          createInvokeCommandClient(vi.fn().mockResolvedValue(payload)),
        ),
      ).rejects.toThrow(TauriCommandPayloadError)
    }
  })

  it('rejects raw RunEvent-shaped payloads as conversation worktree pages', async () => {
    const client = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        events: [
          {
            id: 'evt-live',
            conversationSequence: 2,
            payload: { messageId: 'message-live', text: 'Hello' },
            runId: 'run-001',
            sequence: 2,
            source: 'assistant',
            timestamp: '2026-06-17T00:00:01.000Z',
            type: 'assistant.delta',
            visibility: 'public',
          },
        ],
        cursor: cursor(''),
        gap: false,
      }),
    )

    await expect(
      pageConversationWorktree({ conversationId: 'conversation-001' }, client),
    ).rejects.toThrow(TauriCommandPayloadError)
  })

  it('models artifact media preview command without exposing blob paths', async () => {
    const invoke = vi.fn().mockResolvedValue({
      dataUrl: 'data:image/png;base64,iVBORw0KGgo=',
      mimeType: 'image/png',
      sizeBytes: 67,
    })
    const client = createInvokeCommandClient(invoke)

    await expect(
      getArtifactMediaPreview(
        {
          conversationId: 'conversation-001',
          artifactId: 'artifact-image-001',
        },
        client,
      ),
    ).resolves.toEqual({
      dataUrl: 'data:image/png;base64,iVBORw0KGgo=',
      mimeType: 'image/png',
      sizeBytes: 67,
    })
    expect(invoke).toHaveBeenCalledWith('get_artifact_media_preview', {
      conversationId: 'conversation-001',
      artifactId: 'artifact-image-001',
    })

    const unsafeClient = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        dataUrl: '/Users/goya/.jyowo/runtime/blobs/private.png',
        mimeType: 'image/png',
        sizeBytes: 67,
      }),
    )
    await expect(
      getArtifactMediaPreview(
        {
          conversationId: 'conversation-001',
          artifactId: 'artifact-image-001',
        },
        unsafeClient,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
  })

  it('rejects svg artifact media preview payloads', async () => {
    const client = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        dataUrl: 'data:image/svg+xml;base64,PHN2Zy8+',
        mimeType: 'image/svg+xml',
        sizeBytes: 12,
      }),
    )

    await expect(
      getArtifactMediaPreview(
        {
          conversationId: 'conversation-001',
          artifactId: 'artifact-image-001',
        },
        client,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
  })

  it('rejects artifact media preview payloads when data URL MIME differs from mimeType', async () => {
    const client = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        dataUrl: 'data:image/jpeg;base64,iVBORw0KGgo=',
        mimeType: 'image/png',
        sizeBytes: 67,
      }),
    )

    await expect(
      getArtifactMediaPreview(
        {
          conversationId: 'conversation-001',
          artifactId: 'artifact-image-001',
        },
        client,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
  })

  it('models attachment media preview command without exposing blob paths', async () => {
    const invoke = vi.fn().mockResolvedValue({
      dataUrl: 'data:image/png;base64,iVBORw0KGgo=',
      mimeType: 'image/png',
      sizeBytes: 67,
    })
    const client = createInvokeCommandClient(invoke)

    await expect(
      getAttachmentMediaPreview(
        {
          conversationId: 'conversation-001',
          attachmentId: attachmentPreviewId,
        },
        client,
      ),
    ).resolves.toEqual({
      dataUrl: 'data:image/png;base64,iVBORw0KGgo=',
      mimeType: 'image/png',
      sizeBytes: 67,
    })
    expect(invoke).toHaveBeenCalledWith('get_attachment_media_preview', {
      conversationId: 'conversation-001',
      attachmentId: attachmentPreviewId,
    })

    const unsafeClient = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        dataUrl: '/Users/goya/.jyowo/runtime/blobs/private.png',
        mimeType: 'image/png',
        sizeBytes: 67,
      }),
    )
    await expect(
      getAttachmentMediaPreview(
        {
          conversationId: 'conversation-001',
          attachmentId: attachmentPreviewId,
        },
        unsafeClient,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
  })

  it('rejects non-image attachment media preview data URLs', async () => {
    const client = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        dataUrl: 'data:text/plain;base64,aGVsbG8=',
        mimeType: 'image/png',
        sizeBytes: 5,
      }),
    )

    await expect(
      getAttachmentMediaPreview(
        {
          conversationId: 'conversation-001',
          attachmentId: attachmentPreviewId,
        },
        client,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
  })

  it('rejects attachment media preview responses with mismatched MIME metadata', async () => {
    const client = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        dataUrl: 'data:image/png;base64,iVBORw0KGgo=',
        mimeType: 'image/avif',
        sizeBytes: 67,
      }),
    )

    await expect(
      getAttachmentMediaPreview(
        {
          conversationId: 'conversation-001',
          attachmentId: attachmentPreviewId,
        },
        client,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
  })

  it('models run and permission intent commands without exposing generic execute', async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === 'start_run') {
        return { runId: 'run-001', status: 'started' }
      }

      if (command === 'cancel_run') {
        return { runId: 'run-001', status: 'cancelled' }
      }

      return {
        decision: 'approve',
        requestId: '01HZ0000000000000000000001',
        status: 'resolved',
      }
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
          modelConfigId: 'provider-config-001',
          permissionMode: 'bypass_permissions',
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
          optionId: '01HZ0000000000000000000002',
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
      modelConfigId: 'provider-config-001',
      permissionMode: 'bypass_permissions',
      prompt: 'Continue implementation',
    })
    expect(invoke).toHaveBeenCalledWith('cancel_run', { runId: 'run-001' })
    expect(invoke).toHaveBeenCalledWith('resolve_permission', {
      conversationId: 'conversation-001',
      decision: 'approve',
      optionId: '01HZ0000000000000000000002',
      requestId: '01HZ0000000000000000000001',
    })
    expect(invoke).not.toHaveBeenCalledWith('execute', expect.anything())
  })

  it('passes confirmationText through resolve permission IPC', async () => {
    const invoke = vi.fn().mockResolvedValue({
      decision: 'approve',
      requestId: '01HZ0000000000000000000001',
      status: 'resolved',
    })
    const client = createInvokeCommandClient(invoke)

    await expect(
      resolvePermission(
        {
          confirmationText: 'DELETE',
          conversationId: 'conversation-001',
          decision: 'approve',
          optionId: '01HZ0000000000000000000002',
          requestId: '01HZ0000000000000000000001',
        },
        client,
      ),
    ).resolves.toEqual({
      decision: 'approve',
      requestId: '01HZ0000000000000000000001',
      status: 'resolved',
    })
    expect(invoke).toHaveBeenCalledWith('resolve_permission', {
      confirmationText: 'DELETE',
      conversationId: 'conversation-001',
      decision: 'approve',
      optionId: '01HZ0000000000000000000002',
      requestId: '01HZ0000000000000000000001',
    })
  })

  it('rejects empty confirmationText before invoking IPC', async () => {
    const invoke = vi.fn()
    const client = createInvokeCommandClient(invoke)

    await expect(
      resolvePermission(
        {
          confirmationText: '',
          conversationId: 'conversation-001',
          decision: 'approve',
          optionId: '01HZ0000000000000000000002',
          requestId: '01HZ0000000000000000000001',
        },
        client,
      ),
    ).rejects.toBeInstanceOf(TauriCommandPayloadError)
    expect(invoke).not.toHaveBeenCalled()
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
              payload: { messageId: 'message-replay', text: 'Hello' },
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
    tauriListenSpy.mockImplementationOnce(async (_eventName, handler) => {
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
    expect(tauriListenSpy).toHaveBeenCalledWith('conversation_event_batch', expect.any(Function))
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

  it('emits fixture permission requests with production-compatible ids', async () => {
    const client = createTestCommandClient()
    const permissionRequest = new Promise<string>((resolve) => {
      void client.listenConversationEventBatches((batch) => {
        const permissionEvent = batch.events.find((event) => event.type === 'permission.requested')
        if (permissionEvent?.type === 'permission.requested' && permissionEvent.payload) {
          resolve(permissionEvent.payload.requestId)
        }
      })
    })

    await client.subscribeConversationEvents({
      conversationId: 'conversation-001',
    })
    await client.startRun({
      attachments: [],
      clientMessageId: '00000000-0000-4000-8000-000000000001',
      contextReferences: [],
      conversationId: 'conversation-001',
      modelConfigId: 'provider-config-001',
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
          modelConfigId: 'provider-config-001',
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
          modelConfigId: 'provider-config-001',
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
          modelConfigId: 'provider-config-001',
          prompt: 'Continue',
        },
        client,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
    await expect(
      startRun(
        {
          conversationId: 'conversation-001',
          modelConfigId: 'provider-config-001',
          permissionMode: 'ask' as never,
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
          modelConfigId: 'provider-config-001',
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

    expect(invoke).toHaveBeenCalledWith('create_attachment_from_path', {
      path: '/tmp/notes.txt',
    })
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
          optionId: '01HZ0000000000000000000002',
          requestId: '01HZ0000000000000000000001',
        } as unknown as Parameters<typeof resolvePermission>[0],
        client,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
    await expect(
      resolvePermission(
        {
          conversationId: 'conversation-001',
          decision: 'approve',
          optionId: '01HZ0000000000000000000002',
          requestId: ' ',
        },
        client,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
    await expect(
      resolvePermission(
        {
          conversationId: 'conversation-001',
          decision: 'approve',
          optionId: '01HZ0000000000000000000002',
          requestId: '01hz0000000000000000000001',
        },
        client,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
    await expect(
      resolvePermission(
        {
          conversationId: 'conversation-001',
          decision: 'approve',
          requestId: '01HZ0000000000000000000001',
        } as unknown as Parameters<typeof resolvePermission>[0],
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
              payload: {
                sessionId: 'session-001',
                model: openAiRunModelSnapshot,
              },
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
        decisions: [
          {
            detail: 'Before runtime integration',
            title: 'Review IPC boundary',
          },
        ],
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

  it('rejects unsafe artifact metadata display references', async () => {
    for (const unsafeFields of [
      { actionLabel: 'Open data:text/plain,secret' },
      { actionLabel: 'Open sk-abcdefghijklmnopqrstuvwxyz' },
      { description: 'Generated .JYOWO/runtime/blobs/blob-001' },
      { description: 'Generated token=provider-secret' },
      { kind: 'markdown data:image/svg+xml,<svg onload=alert(1)>' },
      { title: 'Generated javascript:alert(1)' },
      { title: 'Generated sk-abcdefghijklmnopqrstuvwxyz' },
      { preview: 'Blob:.jyowo/runtime/blobs/blob-001 log/tmp/provider-output' },
      { preview: 'Blob:.JYOWO/runtime/blobs/blob-001' },
      { preview: 'Opaque blob:null/provider-output' },
      { preview: 'token=provider-secret' },
    ]) {
      const client = createInvokeCommandClient(
        vi.fn().mockResolvedValue({
          artifacts: [
            {
              actionLabel: 'Open',
              description: 'Generated implementation plan',
              id: 'artifact-foundation-plan',
              kind: 'markdown',
              status: 'ready',
              title: 'Foundation implementation review',
              ...unsafeFields,
            },
          ],
        }),
      )

      await expect(listArtifacts({ conversationId: 'conversation-001' }, client)).rejects.toThrow(
        TauriCommandPayloadError,
      )
    }
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
    expect(invoke).toHaveBeenCalledWith('run_eval_case', {
      caseId: 'regression-smoke',
    })
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
      vi.fn().mockResolvedValue({
        conversations: [{ id: '', title: '', updatedAt: 'not-a-date' }],
      }),
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

  it('rejects Windows slash private paths in conversation IPC payloads', async () => {
    const client = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        conversations: [
          {
            id: 'conversation-001',
            isEmpty: false,
            lastMessagePreview: 'read C:/Users/goya/.ssh/config',
            title: 'read C:/Users/goya/.ssh/config',
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

    await expect(
      startRun({ conversationId: '', modelConfigId: '', prompt: '' }, client),
    ).rejects.toThrow(TauriCommandPayloadError)
    await expect(deleteConversation('', client)).rejects.toThrow(TauriCommandPayloadError)
    await expect(deleteProject('', client)).rejects.toThrow(TauriCommandPayloadError)
    expect(invoke).not.toHaveBeenCalled()
  })

  it('supports explicit fixture behavior for conversation commands', async () => {
    const client = createTestCommandClient()

    await expect(listConversations(client)).resolves.toHaveProperty('conversations')
    await expect(createConversation(client)).resolves.toHaveProperty('conversation.id')
    await expect(getConversation('conversation-001', client)).resolves.toHaveProperty(
      'conversation.id',
      'conversation-001',
    )
    await expect(
      startRun(
        {
          conversationId: 'conversation-001',
          modelConfigId: 'provider-config-001',
          prompt: 'Run',
        },
        client,
      ),
    ).resolves.toHaveProperty('status', 'started')
    await expect(deleteConversation('conversation-001', client)).resolves.toHaveProperty(
      'status',
      'deleted',
    )
    await expect(deleteProject('/Users/goya/Repo/Git/Jyowo', client)).resolves.toHaveProperty(
      'status',
      'deleted',
    )
    await expect(cancelRun('run-001', client)).resolves.toHaveProperty('status', 'cancelled')
    await expect(
      resolvePermission(
        {
          conversationId: 'conversation-001',
          decision: 'deny',
          optionId: '01HZ0000000000000000000002',
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
                  {
                    id: 'default',
                    label: 'Default',
                    baseUrl: 'https://api.openai.com',
                  },
                ],
                supportsLiveValidation: false,
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
            hasOfficialQuotaApiKey: false,
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
            supportsLiveValidation: false,
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
          hasOfficialQuotaApiKey: false,
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
    const officialQuotaToken = 'official-quota-token'
    const invoke = vi.fn(async (command: string) => {
      if (command === 'validate_provider_settings') {
        return {
          modelId: 'gpt-5.4-mini',
          providerId: 'openai',
          status: 'accepted',
        }
      }
      if (command === 'request_provider_config_api_key_reveal') {
        return {
          configId: 'openai',
          expiresInSeconds: 60,
          revealToken: 'reveal-token',
          status: 'ready',
        }
      }
      if (command === 'get_provider_config_api_key') {
        return {
          apiKey: providerToken,
          configId: 'openai',
        }
      }

      return {
        config: {
          protocol: 'responses',
          baseUrl: 'https://gateway.example.com',
          displayName: 'OpenAI gateway',
          hasApiKey: true,
          hasOfficialQuotaApiKey: true,
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
          officialQuotaApiKey: officialQuotaToken,
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
        hasOfficialQuotaApiKey: true,
        id: 'openai',
        isDefault: true,
        modelDescriptor: openAiModelDescriptor,
        modelId: 'gpt-5.4-mini',
        providerId: 'openai',
      },
      status: 'saved',
    })

    await expect(requestProviderConfigApiKeyReveal('openai', client)).resolves.toEqual({
      configId: 'openai',
      expiresInSeconds: 60,
      revealToken: 'reveal-token',
      status: 'ready',
    })
    await expect(getProviderConfigApiKey('openai', 'reveal-token', client)).resolves.toEqual({
      apiKey: providerToken,
      configId: 'openai',
    })

    expect(JSON.stringify(invoke.mock.results.slice(0, 2))).not.toContain(providerToken)
    expect(JSON.stringify(invoke.mock.results.slice(0, 2))).not.toContain(officialQuotaToken)
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
      officialQuotaApiKey: officialQuotaToken,
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

  it('returns fixture provider API key reveal after save without storing raw keys in list data', async () => {
    const client = createTestCommandClient()
    const providerToken = 'provider-test-token'

    await client.saveProviderSettings({
      apiKey: providerToken,
      baseUrl: 'https://api.openai.com',
      modelId: 'gpt-4o-mini',
      providerId: 'openai',
      setDefault: true,
    })
    const reveal = await client.requestProviderConfigApiKeyReveal('openai')
    expect(reveal).toMatchObject({
      configId: 'openai',
      expiresInSeconds: 60,
      status: 'ready',
    })
    expect(reveal.revealToken).toMatch(/^fixture-reveal-token-\d+$/)
    await expect(client.getProviderConfigApiKey('openai', reveal.revealToken)).resolves.toEqual({
      apiKey: expect.any(String),
      configId: 'openai',
    })
    await expect(client.getProviderConfigApiKey('openai', reveal.revealToken)).rejects.toThrow(
      'invalid or expired',
    )

    const mismatchReveal = await client.requestProviderConfigApiKeyReveal('openai')
    await expect(
      client.getProviderConfigApiKey('openai-personal', mismatchReveal.revealToken),
    ).rejects.toThrow('invalid or expired')
    await expect(
      client.getProviderConfigApiKey('openai', mismatchReveal.revealToken),
    ).rejects.toThrow('invalid or expired')
    await expect(client.requestProviderConfigApiKeyReveal('unknown')).rejects.toThrow(
      'not configured',
    )
    expect(JSON.stringify(await client.listProviderSettings())).not.toContain(providerToken)
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
    await expect(
      saveProviderSettings(
        {
          modelId: 'gpt-5.4-mini',
          official_quota_api_key: 'snake-case-token',
          providerId: 'openai',
        } as unknown as Parameters<typeof saveProviderSettings>[0],
        client,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
    expect(invoke).not.toHaveBeenCalled()
  })

  it('parses provider probe responses and rejects malformed probe payloads', async () => {
    const validSnapshot = {
      checkedAt: '2026-06-30T12:00:00+00:00',
      configId: 'openai-work',
      modelId: 'gpt-5.4-mini',
      providerId: 'openai',
      status: 'online',
      timeoutMs: 10_000,
      latencyMs: 120,
    }
    const invoke = vi.fn().mockResolvedValue({
      snapshot: validSnapshot,
      diagnosticUsage: {
        cacheReadTokens: 0,
        cacheWriteTokens: 0,
        costMicros: 0,
        inputTokens: 12,
        outputTokens: 3,
        toolCalls: 0,
      },
    })
    const client = createInvokeCommandClient(invoke)

    await expect(
      probeProviderConfig({ configId: 'openai-work', timeoutMs: 10_000 }, client),
    ).resolves.toEqual({
      snapshot: validSnapshot,
      diagnosticUsage: {
        cacheReadTokens: 0,
        cacheWriteTokens: 0,
        costMicros: 0,
        inputTokens: 12,
        outputTokens: 3,
        toolCalls: 0,
      },
    })
    expect(invoke).toHaveBeenCalledWith('probe_provider_config', {
      configId: 'openai-work',
      timeoutMs: 10_000,
    })

    invoke.mockResolvedValueOnce({
      snapshots: [validSnapshot],
    })
    await expect(listProviderProbeSnapshots(client)).resolves.toEqual({
      snapshots: [validSnapshot],
    })
    expect(invoke).toHaveBeenCalledWith('list_provider_probe_snapshots')

    const rejectClient = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        snapshot: {
          ...validSnapshot,
          config_id: 'openai-work',
        },
      }),
    )
    await expect(probeProviderConfig({ configId: 'openai-work' }, rejectClient)).rejects.toThrow(
      TauriCommandPayloadError,
    )

    const neverCheckedClient = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        snapshot: {
          ...validSnapshot,
          status: 'never_checked',
        },
      }),
    )
    await expect(
      probeProviderConfig({ configId: 'openai-work' }, neverCheckedClient),
    ).rejects.toThrow(TauriCommandPayloadError)

    const missingCheckedAtClient = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        snapshot: {
          configId: 'openai-work',
          modelId: 'gpt-5.4-mini',
          providerId: 'openai',
          status: 'online',
          timeoutMs: 10_000,
        },
      }),
    )
    await expect(
      probeProviderConfig({ configId: 'openai-work' }, missingCheckedAtClient),
    ).rejects.toThrow(TauriCommandPayloadError)

    const negativeLatencyClient = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        snapshot: {
          ...validSnapshot,
          latencyMs: -1,
        },
      }),
    )
    await expect(
      probeProviderConfig({ configId: 'openai-work' }, negativeLatencyClient),
    ).rejects.toThrow(TauriCommandPayloadError)

    const unknownErrorKindClient = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        snapshot: {
          ...validSnapshot,
          errorKind: 'secret_leak',
        },
      }),
    )
    await expect(
      probeProviderConfig({ configId: 'openai-work' }, unknownErrorKindClient),
    ).rejects.toThrow(TauriCommandPayloadError)
  })

  it('rejects invalid provider probe requests before invoking Tauri', async () => {
    const invoke = vi.fn()
    const client = createInvokeCommandClient(invoke)

    await expect(probeProviderConfig({ configId: '   ' }, client)).rejects.toThrow(
      TauriCommandPayloadError,
    )
    expect(invoke).not.toHaveBeenCalled()
  })

  it('parses model usage summary responses with timezone identity fields', async () => {
    const usageWindow = {
      period: 'today' as const,
      periodStart: '2026-06-30T00:00:00Z',
      periodEnd: '2026-06-30T15:00:00Z',
      total: {
        cacheReadTokens: 1,
        cacheWriteTokens: 2,
        costMicros: 100,
        inputTokens: 10,
        outputTokens: 5,
        toolCalls: 3,
      },
      byModel: [
        {
          key: 'openai/gpt-4.1',
          providerId: 'openai',
          modelId: 'gpt-4.1',
          usage: {
            cacheReadTokens: 1,
            cacheWriteTokens: 2,
            costMicros: 100,
            inputTokens: 10,
            outputTokens: 5,
            toolCalls: 3,
          },
          lastUsedAt: '2026-06-30T10:00:00Z',
        },
      ],
    }
    const invoke = vi.fn().mockResolvedValue({
      timezoneId: 'America/New_York',
      timezoneOffsetMinutes: -240,
      today: usageWindow,
      monthToDate: { ...usageWindow, period: 'month_to_date' },
      allTime: {
        ...usageWindow,
        period: 'all_time',
        periodStart: undefined,
        periodEnd: undefined,
      },
      generatedAt: '2026-06-30T15:00:00Z',
    })
    const client = createInvokeCommandClient(invoke)

    await expect(getModelUsageSummary(client)).resolves.toEqual({
      timezoneId: 'America/New_York',
      timezoneOffsetMinutes: -240,
      today: usageWindow,
      monthToDate: { ...usageWindow, period: 'month_to_date' },
      allTime: {
        ...usageWindow,
        period: 'all_time',
        periodStart: undefined,
        periodEnd: undefined,
      },
      generatedAt: '2026-06-30T15:00:00Z',
    })
    expect(invoke).toHaveBeenCalledWith('get_model_usage_summary')

    const allTimeOnlyClient = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        total: usageWindow.total,
        byModel: usageWindow.byModel,
      }),
    )
    await expect(getModelUsageSummary(allTimeOnlyClient)).rejects.toThrow(TauriCommandPayloadError)

    const missingTimezoneClient = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        timezoneId: 'America/New_York',
        today: usageWindow,
        monthToDate: { ...usageWindow, period: 'month_to_date' },
        allTime: { ...usageWindow, period: 'all_time' },
        generatedAt: '2026-06-30T15:00:00Z',
      }),
    )
    await expect(getModelUsageSummary(missingTimezoneClient)).rejects.toThrow(
      TauriCommandPayloadError,
    )
  })

  it('validates official quota snapshots and rejects snake_case backend fields', async () => {
    const snapshot = {
      configId: 'openrouter-work',
      expiresAt: '2026-06-30T12:15:00Z',
      fetchedAt: '2026-06-30T12:00:00Z',
      isStale: false,
      providerId: 'openrouter',
      scope: 'account',
      sourceUrl: 'https://openrouter.ai/docs/api/api-reference/api-keys/get-current-key',
      status: 'supported',
    }
    const invoke = vi.fn().mockResolvedValue({ snapshot })
    const client = createInvokeCommandClient(invoke)

    await expect(refreshOfficialQuota({ configId: 'openrouter-work' }, client)).resolves.toEqual({
      snapshot,
    })
    expect(invoke).toHaveBeenCalledWith('refresh_official_quota', {
      configId: 'openrouter-work',
    })

    await expect(
      listOfficialQuotaSnapshots(
        createInvokeCommandClient(vi.fn().mockResolvedValue({ snapshots: [snapshot] })),
      ),
    ).resolves.toEqual({ snapshots: [snapshot] })

    const unsupportedClient = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        snapshot: {
          ...snapshot,
          status: 'unsupported',
          safeMessage: 'No official quota API.',
        },
      }),
    )
    await expect(
      refreshOfficialQuota({ configId: 'gemini-work' }, unsupportedClient),
    ).resolves.toMatchObject({
      snapshot: {
        status: 'unsupported',
        safeMessage: 'No official quota API.',
      },
    })

    const missingFreshnessClient = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        snapshot: {
          configId: 'openrouter-work',
          providerId: 'openrouter',
          scope: 'account',
          sourceUrl: 'https://openrouter.ai/docs/api/api-reference/api-keys/get-current-key',
          status: 'supported',
        },
      }),
    )
    await expect(
      refreshOfficialQuota({ configId: 'openrouter-work' }, missingFreshnessClient),
    ).rejects.toThrow(TauriCommandPayloadError)

    const snakeCaseClient = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        snapshot: {
          config_id: 'openrouter-work',
          fetched_at: '2026-06-30T12:00:00Z',
          expires_at: '2026-06-30T12:15:00Z',
          is_stale: false,
          provider_id: 'openrouter',
          scope: 'account',
          source_url: 'https://openrouter.ai/docs/api/api-reference/api-keys/get-current-key',
          status: 'supported',
        },
      }),
    )
    await expect(
      refreshOfficialQuota({ configId: 'openrouter-work' }, snakeCaseClient),
    ).rejects.toThrow(TauriCommandPayloadError)

    await expect(refreshOfficialQuota({ configId: '   ' }, client)).rejects.toThrow(
      TauriCommandPayloadError,
    )
  })

  it('parses provider capability route list responses', async () => {
    const invoke = vi.fn().mockResolvedValue({
      version: 1,
      routes: [
        {
          kind: 'image_generation',
          configId: 'minimax-image',
          providerId: 'minimax',
          operationIds: ['minimax.image_generation'],
          enabled: false,
        },
      ],
    })
    const client = createInvokeCommandClient(invoke)

    await expect(listProviderCapabilityRoutes(client)).resolves.toEqual({
      version: 1,
      routes: [
        {
          kind: 'image_generation',
          configId: 'minimax-image',
          providerId: 'minimax',
          operationIds: ['minimax.image_generation'],
          enabled: false,
        },
      ],
    })
    expect(invoke).toHaveBeenCalledWith('list_provider_capability_routes')
  })

  it('parses provider capability route options with runtime support metadata', async () => {
    const invoke = vi.fn().mockResolvedValue({
      options: [
        {
          kind: 'image_generation',
          configId: 'minimax-image',
          providerId: 'minimax',
          operationId: 'minimax.image_generation',
          outputArtifact: 'image',
          execution: 'sync',
          costRisk: 'medium',
          runtimeSupported: false,
          unavailableReason: 'runtime adapter unavailable',
        },
      ],
    })
    const client = createInvokeCommandClient(invoke)

    await expect(listProviderCapabilityRouteOptions(client)).resolves.toEqual({
      options: [
        {
          kind: 'image_generation',
          configId: 'minimax-image',
          providerId: 'minimax',
          operationId: 'minimax.image_generation',
          outputArtifact: 'image',
          execution: 'sync',
          costRisk: 'medium',
          runtimeSupported: false,
          unavailableReason: 'runtime adapter unavailable',
        },
      ],
    })
    expect(invoke).toHaveBeenCalledWith('list_provider_capability_route_options')
  })

  it('rejects unknown provider capability route save fields before invoking Tauri', async () => {
    const invoke = vi.fn()
    const client = createInvokeCommandClient(invoke)

    await expect(
      saveProviderCapabilityRoute(
        {
          route: {
            kind: 'image_generation',
            configId: 'minimax-image',
            providerId: 'minimax',
            operationIds: ['minimax.image_generation'],
            enabled: false,
          },
          unexpectedField: true,
        } as unknown as Parameters<typeof saveProviderCapabilityRoute>[0],
        client,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
    expect(invoke).not.toHaveBeenCalled()
  })

  it('validates provider capability route delete kind before invoking Tauri', async () => {
    const invoke = vi.fn()
    const client = createInvokeCommandClient(invoke)

    await expect(
      deleteProviderCapabilityRoute(
        {
          kind: 'invalid_kind',
          configId: 'minimax-image',
          providerId: 'minimax',
        } as unknown as Parameters<typeof deleteProviderCapabilityRoute>[0],
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
              enabled: true,
              exposedToolCount: 2,
              id: 'github',
              manageable: true,
              origin: 'workspace',
              scope: 'global',
              status: 'ready',
              transport: 'stdio',
            },
            {
              displayName: 'Plugin Context',
              enabled: true,
              exposedToolCount: 1,
              id: 'plugin-context',
              manageable: false,
              origin: 'plugin',
              scope: 'session',
              sourcePluginId: 'formatter@1.0.0',
              status: 'ready',
              transport: 'http',
            },
          ],
        }
      }

      if (command === 'get_mcp_server_config') {
        return {
          server: {
            displayName: 'Workspace GitHub',
            enabled: true,
            id: 'github',
            scope: 'global',
            transport: {
              args: ['mcp-server'],
              command: 'node',
              env: [{ key: 'LOG_LEVEL', value: 'info' }],
              inheritEnv: ['GITHUB_TOKEN'],
              kind: 'stdio',
            },
          },
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
          enabled: true,
          exposedToolCount: 0,
          id: 'github',
          manageable: true,
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
          enabled: true,
          exposedToolCount: 2,
          id: 'github',
          manageable: true,
          origin: 'workspace',
          scope: 'global',
          status: 'ready',
          transport: 'stdio',
        },
        {
          displayName: 'Plugin Context',
          enabled: true,
          exposedToolCount: 1,
          id: 'plugin-context',
          manageable: false,
          origin: 'plugin',
          scope: 'session',
          sourcePluginId: 'formatter@1.0.0',
          status: 'ready',
          transport: 'http',
        },
      ],
    })
    await expect(getMcpServerConfig('github', client)).resolves.toEqual({
      server: {
        displayName: 'Workspace GitHub',
        enabled: true,
        id: 'github',
        scope: 'global',
        transport: {
          args: ['mcp-server'],
          command: 'node',
          env: [{ key: 'LOG_LEVEL', value: 'info' }],
          inheritEnv: ['GITHUB_TOKEN'],
          kind: 'stdio',
        },
      },
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
    expect(invoke).toHaveBeenCalledWith('get_mcp_server_config', {
      id: 'github',
    })
    expect(invoke).toHaveBeenCalledWith('save_mcp_server', {
      displayName: 'Workspace GitHub',
      enabled: true,
      id: 'github',
      scope: 'global',
      transport: {
        args: ['mcp-server'],
        command: 'node',
        env: [],
        inheritEnv: [],
        kind: 'stdio',
      },
    })
    expect(invoke).toHaveBeenCalledWith('delete_mcp_server', { id: 'github' })
  })

  it('models browser MCP presets as disabled explicit MCP server configs', async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === 'list_browser_mcp_presets') {
        return {
          presets: [
            {
              description: 'Browser automation through Playwright MCP.',
              displayName: 'Playwright Browser',
              enabled: false,
              id: 'playwright',
              serverId: 'browser-playwright',
            },
            {
              description: 'Browser inspection through Chrome DevTools MCP.',
              displayName: 'Chrome DevTools Browser',
              enabled: false,
              id: 'chrome-devtools',
              serverId: 'browser-chrome-devtools',
            },
          ],
        }
      }

      return {
        preset: {
          description: 'Browser automation through Playwright MCP.',
          displayName: 'Playwright Browser',
          enabled: false,
          id: 'playwright',
          serverId: 'browser-playwright',
        },
        server: {
          displayName: 'Playwright Browser',
          enabled: false,
          exposedToolCount: 0,
          id: 'browser-playwright',
          manageable: true,
          origin: 'workspace',
          scope: 'global',
          status: 'disabled',
          transport: 'stdio',
        },
      }
    })
    const client = createInvokeCommandClient(invoke)

    await expect(listBrowserMcpPresets(client)).resolves.toEqual({
      presets: [
        {
          description: 'Browser automation through Playwright MCP.',
          displayName: 'Playwright Browser',
          enabled: false,
          id: 'playwright',
          serverId: 'browser-playwright',
        },
        {
          description: 'Browser inspection through Chrome DevTools MCP.',
          displayName: 'Chrome DevTools Browser',
          enabled: false,
          id: 'chrome-devtools',
          serverId: 'browser-chrome-devtools',
        },
      ],
    })
    await expect(
      saveBrowserMcpPreset({ enabled: false, presetId: 'playwright' }, client),
    ).resolves.toHaveProperty('server.status', 'disabled')

    expect(invoke).toHaveBeenCalledWith('list_browser_mcp_presets')
    expect(invoke).toHaveBeenCalledWith('save_browser_mcp_preset', {
      enabled: false,
      presetId: 'playwright',
    })
    expect(JSON.stringify(invoke.mock.calls)).not.toContain('token')
    expect(JSON.stringify(invoke.mock.calls)).not.toContain('cookie')
  })

  it('accepts MCP stdio and HTTP request shapes without storing raw secret values', async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === 'save_mcp_server') {
        return {
          server: {
            displayName: 'Remote Context',
            enabled: true,
            exposedToolCount: 0,
            id: 'context7',
            manageable: true,
            origin: 'workspace',
            scope: 'global',
            status: 'configured',
            transport: 'http',
          },
        }
      }

      return {
        server: {
          displayName: 'Workspace GitHub',
          enabled: true,
          exposedToolCount: 1,
          id: 'github',
          manageable: true,
          origin: 'workspace',
          scope: 'global',
          status: 'ready',
          transport: 'stdio',
        },
      }
    })
    const client = createInvokeCommandClient(invoke)

    await expect(
      saveMcpServer(
        {
          displayName: 'Remote Context',
          id: 'context7',
          scope: 'global',
          transport: {
            bearerTokenEnvVar: 'MCP_BEARER_TOKEN',
            headers: [{ key: 'X-Workspace', value: 'jyowo' }],
            headersFromEnv: [{ key: 'X-Api-Key', envVar: 'MCP_CONTEXT7_TOKEN' }],
            kind: 'http',
            url: 'https://mcp.example.com/mcp',
          },
        },
        client,
      ),
    ).resolves.toHaveProperty('server.transport', 'http')
    await expect(setMcpServerEnabled('github', true, client)).resolves.toHaveProperty(
      'server.status',
      'ready',
    )
    await expect(restartMcpServer('github', client)).resolves.toHaveProperty(
      'server.transport',
      'stdio',
    )

    expect(JSON.stringify(invoke.mock.calls)).not.toContain('mcp-secret-token')
    expect(invoke).toHaveBeenCalledWith('save_mcp_server', {
      displayName: 'Remote Context',
      enabled: true,
      id: 'context7',
      scope: 'global',
      transport: {
        bearerTokenEnvVar: 'MCP_BEARER_TOKEN',
        headers: [{ key: 'X-Workspace', value: 'jyowo' }],
        headersFromEnv: [{ key: 'X-Api-Key', envVar: 'MCP_CONTEXT7_TOKEN' }],
        kind: 'http',
        url: 'https://mcp.example.com/mcp',
      },
    })
    expect(invoke).toHaveBeenCalledWith('set_mcp_server_enabled', {
      enabled: true,
      id: 'github',
    })
    expect(invoke).toHaveBeenCalledWith('restart_mcp_server', { id: 'github' })
  })

  it('rejects raw secret MCP headers and stdio env before invoking Tauri', async () => {
    const invoke = vi.fn()
    const client = createInvokeCommandClient(invoke)

    await expect(
      saveMcpServer(
        {
          displayName: 'Remote Context',
          id: 'context7',
          scope: 'global',
          transport: {
            headers: [{ key: 'Authorization', value: 'Bearer mcp-secret-token' }],
            kind: 'http',
            url: 'https://mcp.example.com/mcp',
          },
        } as Parameters<typeof saveMcpServer>[0],
        client,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
    await expect(
      saveMcpServer(
        {
          displayName: 'Workspace GitHub',
          id: 'github',
          scope: 'global',
          transport: {
            command: 'node',
            env: [{ key: 'GITHUB_TOKEN', value: 'mcp-secret-token' }],
            kind: 'stdio',
          },
        } as Parameters<typeof saveMcpServer>[0],
        client,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
    expect(invoke).not.toHaveBeenCalled()
  })

  it('rejects MCP server config details that contain raw secrets', async () => {
    const invoke = vi.fn(async () => ({
      server: {
        displayName: 'Remote Context',
        enabled: true,
        id: 'context7',
        scope: 'global',
        transport: {
          headers: [{ key: 'Authorization', value: 'Bearer mcp-secret-token' }],
          kind: 'http',
          url: 'https://mcp.example.com/mcp',
        },
      },
    }))
    const client = createInvokeCommandClient(invoke)

    await expect(getMcpServerConfig('context7', client)).rejects.toThrow(TauriCommandPayloadError)
  })

  it('parses MCP diagnostics list and live batches without exposing raw payload details', async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === 'list_mcp_diagnostics') {
        return {
          events: [
            {
              eventType: 'connection_lost',
              id: 'mcp-diagnostic-001',
              serverId: 'github',
              severity: 'warning',
              summary: 'MCP server connection lost; reconnecting.',
              timestamp: '2026-06-17T00:00:00.000Z',
            },
          ],
        }
      }

      if (command === 'subscribe_mcp_diagnostics') {
        return {
          replayEvents: [],
          serverId: 'github',
          subscriptionId: 'mcp-diagnostic-subscription-001',
        }
      }

      if (command === 'clear_mcp_diagnostics') {
        return { status: 'cleared' }
      }

      return {
        status: 'unsubscribed',
        subscriptionId: 'mcp-diagnostic-subscription-001',
      }
    })
    const unlisten = vi.fn()
    let tauriEventHandler: ((event: { payload: unknown }) => void) | undefined
    tauriListenSpy.mockImplementationOnce(async (_eventName, handler) => {
      tauriEventHandler = handler
      return unlisten
    })
    const client = createInvokeCommandClient(invoke)
    const batches: unknown[] = []

    await expect(listMcpDiagnostics('github', client)).resolves.toHaveProperty(
      'events.0.summary',
      'MCP server connection lost; reconnecting.',
    )
    await expect(subscribeMcpDiagnostics({ serverId: 'github' }, client)).resolves.toHaveProperty(
      'subscriptionId',
      'mcp-diagnostic-subscription-001',
    )
    const cleanup = await listenMcpDiagnosticBatches((batch) => {
      batches.push(batch)
    }, client)
    tauriEventHandler?.({
      payload: {
        events: [
          {
            eventType: 'connection_recovered',
            id: 'mcp-diagnostic-002',
            serverId: 'github',
            severity: 'info',
            summary: 'MCP server connection recovered.',
            timestamp: '2026-06-17T00:00:01.000Z',
          },
        ],
        phase: 'live',
        serverId: 'github',
        subscriptionId: 'mcp-diagnostic-subscription-001',
      },
    })
    cleanup()
    await expect(clearMcpDiagnostics('github', client)).resolves.toEqual({
      status: 'cleared',
    })
    await expect(
      unsubscribeMcpDiagnostics('mcp-diagnostic-subscription-001', client),
    ).resolves.toHaveProperty('status', 'unsubscribed')

    expect(tauriListenSpy).toHaveBeenCalledWith('mcp_diagnostic_batch', expect.any(Function))
    expect(JSON.stringify(batches)).not.toContain('mcp-secret-token')
    expect(batches).toEqual([
      expect.objectContaining({
        events: [expect.objectContaining({ id: 'mcp-diagnostic-002' })],
        phase: 'live',
      }),
    ])
    expect(unlisten).toHaveBeenCalledTimes(1)
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
            {
              description: 'Formats workspace files.',
              enabled: true,
              id: 'format-file',
              manageable: false,
              name: 'format-file',
              sourceKind: 'plugin',
              sourcePluginId: 'formatter@1.0.0',
              status: 'ready',
              tags: ['formatting'],
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
    await expect(listSkills(client)).resolves.toHaveProperty(
      'skills.1.sourcePluginId',
      'formatter@1.0.0',
    )
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
    expect(invoke).toHaveBeenCalledWith('import_skill', {
      sourcePath: '/tmp/release-notes',
    })
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

  it('models plugin management commands through strict parsed payloads', async () => {
    const summary = {
      id: 'formatter@1.0.0',
      name: 'formatter',
      version: '1.0.0',
      description: 'Formats workspace files.',
      source: 'user',
      trustLevel: 'user_controlled',
      enabled: true,
      state: 'activated',
      capabilities: [
        {
          kind: 'tool',
          name: 'format_file',
          destructive: false,
          registered: true,
        },
      ],
      warnings: [],
    } as const
    const disabledSummary = {
      ...summary,
      enabled: false,
      state: { disabled: { last_state: 'validated' } },
    } as const
    const installReport = {
      sourcePath: '/tmp/formatter-plugin',
      valid: true,
      summary,
      warnings: [],
    } as const
    const operation = {
      pluginId: summary.id,
      status: 'installed',
      summary,
      report: installReport,
    } as const
    const invoke = vi.fn(async (command: string) => {
      if (command === 'list_plugins') {
        return {
          allowProjectPlugins: true,
          plugins: [summary, disabledSummary],
        }
      }

      if (command === 'get_plugin_detail') {
        return {
          plugin: {
            summary,
            manifestOrigin: {
              file: {
                path: '/tmp/formatter-plugin/plugin.json',
              },
            },
            manifestHash: Array.from({ length: 32 }, () => 7),
            manifest: {
              name: 'formatter',
              version: '1.0.0',
            },
            configurationSchema: {
              type: 'object',
              properties: {
                lineWidth: { type: 'number' },
                apiToken: { type: 'string', secret: true },
              },
            },
            config: {
              lineWidth: 100,
            },
            registeredCapabilities: summary.capabilities,
            recentEvents: ['loaded'],
          },
        }
      }

      if (command === 'validate_plugin_from_path') {
        return installReport
      }

      if (command === 'set_plugin_enabled') {
        return {
          pluginId: summary.id,
          status: 'disabled',
          summary: disabledSummary,
        }
      }

      if (command === 'update_plugin_config') {
        return {
          pluginId: summary.id,
          status: 'configured',
          summary,
        }
      }

      if (command === 'uninstall_plugin') {
        return {
          pluginId: summary.id,
          status: 'uninstalled',
        }
      }

      if (command === 'reload_plugin') {
        return {
          pluginId: summary.id,
          status: 'reloaded',
          summary,
        }
      }

      if (command === 'set_project_plugins_enabled') {
        return {
          allowProjectPlugins: true,
        }
      }

      return operation
    })
    const client = createInvokeCommandClient(invoke)

    await expect(listPlugins(client)).resolves.toEqual({
      allowProjectPlugins: true,
      plugins: [summary, disabledSummary],
    })
    await expect(getPluginDetail(summary.id, client)).resolves.toMatchObject({
      plugin: {
        summary: { id: summary.id, state: 'activated' },
        manifestOrigin: { file: { path: '/tmp/formatter-plugin/plugin.json' } },
        manifestHash: Array.from({ length: 32 }, () => 7),
        config: { lineWidth: 100 },
      },
    })
    await expect(validatePluginFromPath('/tmp/formatter-plugin', client)).resolves.toEqual(
      installReport,
    )
    await expect(installPluginFromPath('/tmp/formatter-plugin', client)).resolves.toEqual(operation)
    await expect(setPluginEnabled(summary.id, false, client)).resolves.toHaveProperty(
      'summary.state.disabled.last_state',
      'validated',
    )
    await expect(
      updatePluginConfig(summary.id, { lineWidth: 120 }, client),
    ).resolves.toHaveProperty('status', 'configured')
    await expect(uninstallPlugin(summary.id, client)).resolves.toEqual({
      pluginId: summary.id,
      status: 'uninstalled',
    })
    await expect(reloadPlugin(summary.id, client)).resolves.toHaveProperty('status', 'reloaded')
    await expect(setProjectPluginsEnabled(true, client)).resolves.toEqual({
      allowProjectPlugins: true,
    })

    expect(JSON.stringify(invoke.mock.calls)).not.toContain('api-token-value')
    expect(invoke).toHaveBeenCalledWith('list_plugins')
    expect(invoke).toHaveBeenCalledWith('get_plugin_detail', {
      pluginId: summary.id,
    })
    expect(invoke).toHaveBeenCalledWith('validate_plugin_from_path', {
      sourcePath: '/tmp/formatter-plugin',
    })
    expect(invoke).toHaveBeenCalledWith('install_plugin_from_path', {
      sourcePath: '/tmp/formatter-plugin',
    })
    expect(invoke).toHaveBeenCalledWith('set_plugin_enabled', {
      enabled: false,
      pluginId: summary.id,
    })
    expect(invoke).toHaveBeenCalledWith('update_plugin_config', {
      pluginId: summary.id,
      values: { lineWidth: 120 },
    })
    expect(invoke).toHaveBeenCalledWith('uninstall_plugin', {
      pluginId: summary.id,
    })
    expect(invoke).toHaveBeenCalledWith('reload_plugin', {
      pluginId: summary.id,
    })
    expect(invoke).toHaveBeenCalledWith('set_project_plugins_enabled', {
      enabled: true,
    })
  })

  it('rejects invalid plugin command args and unsafe plugin payloads', async () => {
    const invoke = vi.fn().mockResolvedValue({
      plugins: [
        {
          id: 'formatter@1.0.0',
          name: 'formatter',
          version: '1.0.0',
          source: 'remote_marketplace',
          trustLevel: 'user_controlled',
          enabled: true,
          state: 'activated',
          capabilities: [],
          warnings: [],
        },
      ],
    })
    const client = createInvokeCommandClient(invoke)

    await expect(getPluginDetail('', client)).rejects.toThrow(TauriCommandPayloadError)
    await expect(validatePluginFromPath('', client)).rejects.toThrow(TauriCommandPayloadError)
    await expect(installPluginFromPath('', client)).rejects.toThrow(TauriCommandPayloadError)
    await expect(setPluginEnabled('', true, client)).rejects.toThrow(TauriCommandPayloadError)
    await expect(updatePluginConfig('', {}, client)).rejects.toThrow(TauriCommandPayloadError)
    await expect(
      updatePluginConfig('formatter@1.0.0', { apiToken: 'sk-unsafe-secret' }, client),
    ).rejects.toThrow(TauriCommandPayloadError)
    await expect(uninstallPlugin('', client)).rejects.toThrow(TauriCommandPayloadError)
    await expect(reloadPlugin('', client)).rejects.toThrow(TauriCommandPayloadError)
    await expect(listPlugins(client)).rejects.toThrow(TauriCommandPayloadError)

    const invalidProjectClient = createInvokeCommandClient(
      vi.fn().mockResolvedValue({
        allowProjectPlugins: true,
        extra: true,
      }),
    )
    await expect(setProjectPluginsEnabled(true, invalidProjectClient)).rejects.toThrow(
      TauriCommandPayloadError,
    )
  })

  it('invokes skill catalog commands through validated payloads', async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === 'list_skill_catalog_sources') {
        return {
          sources: [
            {
              description: 'Official Anthropic skill repository.',
              id: 'anthropic',
              installable: true,
              label: 'Anthropic Skills',
              trustLevel: 'official',
            },
          ],
        }
      }

      if (command === 'list_skill_catalog_entries') {
        return {
          entries: [
            {
              description: 'Create frontend interfaces.',
              entryId: 'anthropic:frontend-design',
              installable: true,
              installed: false,
              name: 'frontend-design',
              sourceId: 'anthropic',
              sourceLabel: 'Anthropic Skills',
              tags: ['frontend'],
              trustLevel: 'official',
              version: 'main',
            },
          ],
          nextCursor: 'cursor-2',
        }
      }

      if (command === 'get_skill_catalog_entry') {
        return {
          entry: {
            description: 'Create frontend interfaces.',
            entryId: 'anthropic:frontend-design',
            installable: true,
            installed: false,
            name: 'frontend-design',
            sourceId: 'anthropic',
            sourceLabel: 'Anthropic Skills',
            tags: ['frontend'],
            trustLevel: 'official',
            version: 'main',
          },
          files: [{ kind: 'file', path: 'SKILL.md', sizeBytes: 512 }],
          readmePreview: 'Create distinctive frontend interfaces.',
          validation: {
            issueCodes: [],
            issues: [],
            status: 'ready',
          },
        }
      }

      if (command === 'get_skill_catalog_file') {
        return {
          file: {
            content: 'Create distinctive frontend interfaces.',
            path: 'SKILL.md',
            truncated: false,
          },
        }
      }

      if (command === 'list_skill_catalog_install_tasks') {
        return {
          tasks: [
            {
              entryId: 'anthropic:frontend-design',
              operationId: 'catalog-install-001',
              percent: 45,
              sourceId: 'anthropic',
              stage: 'downloading',
              startedAt: '2026-06-28T00:00:00Z',
              status: 'running',
              updatedAt: '2026-06-28T00:00:01Z',
              version: 'main',
            },
          ],
        }
      }

      return {
        task: {
          entryId: 'anthropic:frontend-design',
          operationId: 'catalog-install-001',
          percent: 5,
          sourceId: 'anthropic',
          stage: 'preparing',
          startedAt: '2026-06-28T00:00:00Z',
          status: 'running',
          updatedAt: '2026-06-28T00:00:00Z',
          version: 'main',
        },
      }
    })
    const client = createInvokeCommandClient(invoke)

    await expect(listSkillCatalogSources(client)).resolves.toHaveProperty(
      'sources.0.id',
      'anthropic',
    )
    await expect(
      listSkillCatalogEntries(
        {
          cursor: 'cursor-1',
          limit: 12,
          query: 'front',
          sourceId: 'anthropic',
        },
        client,
      ),
    ).resolves.toHaveProperty('entries.0.entryId', 'anthropic:frontend-design')
    await expect(
      getSkillCatalogEntry(
        {
          entryId: 'anthropic:frontend-design',
          sourceId: 'anthropic',
          version: 'main',
        },
        client,
      ),
    ).resolves.toHaveProperty('validation.status', 'ready')
    await expect(
      getSkillCatalogFile(
        {
          entryId: 'anthropic:frontend-design',
          path: 'SKILL.md',
          sourceId: 'anthropic',
          version: 'main',
        },
        client,
      ),
    ).resolves.toHaveProperty('file.content', 'Create distinctive frontend interfaces.')
    await expect(listSkillCatalogInstallTasks(client)).resolves.toHaveProperty(
      'tasks.0.operationId',
      'catalog-install-001',
    )
    await expect(
      installSkillFromCatalog(
        {
          entryId: 'anthropic:frontend-design',
          operationId: 'catalog-install-001',
          sourceId: 'anthropic',
          version: 'main',
        },
        client,
      ),
    ).resolves.toHaveProperty('task.operationId', 'catalog-install-001')

    expect(invoke).toHaveBeenCalledWith('list_skill_catalog_sources')
    expect(invoke).toHaveBeenCalledWith('list_skill_catalog_entries', {
      cursor: 'cursor-1',
      limit: 12,
      query: 'front',
      sourceId: 'anthropic',
    })
    expect(invoke).toHaveBeenCalledWith('get_skill_catalog_entry', {
      entryId: 'anthropic:frontend-design',
      sourceId: 'anthropic',
      version: 'main',
    })
    expect(invoke).toHaveBeenCalledWith('get_skill_catalog_file', {
      entryId: 'anthropic:frontend-design',
      path: 'SKILL.md',
      sourceId: 'anthropic',
      version: 'main',
    })
    expect(invoke).toHaveBeenCalledWith('list_skill_catalog_install_tasks')
    expect(invoke).toHaveBeenCalledWith('install_skill_from_catalog', {
      entryId: 'anthropic:frontend-design',
      operationId: 'catalog-install-001',
      sourceId: 'anthropic',
      version: 'main',
    })
  })

  it('listens to validated skill catalog install progress events', async () => {
    const invoke = vi.fn()
    const unlisten = vi.fn()
    let tauriEventHandler: ((event: { payload: unknown }) => void) | undefined
    tauriListenSpy.mockImplementationOnce(async (_eventName, handler) => {
      tauriEventHandler = handler
      return unlisten
    })
    const client = createInvokeCommandClient(invoke)
    const progressEvents: unknown[] = []

    const cleanup = await listenSkillCatalogInstallProgress((progress) => {
      progressEvents.push(progress)
    }, client)
    tauriEventHandler?.({
      payload: {
        entryId: 'anthropic:frontend-design',
        operationId: 'catalog-install-001',
        percent: 45,
        sourceId: 'anthropic',
        stage: 'downloading',
        version: 'main',
      },
    })
    cleanup()

    expect(tauriListenSpy).toHaveBeenCalledWith(
      'skill_catalog_install_progress',
      expect.any(Function),
    )
    expect(progressEvents).toEqual([
      {
        entryId: 'anthropic:frontend-design',
        operationId: 'catalog-install-001',
        percent: 45,
        sourceId: 'anthropic',
        stage: 'downloading',
        version: 'main',
      },
    ])
    expect(unlisten).toHaveBeenCalledTimes(1)
  })

  it('rejects invalid skill catalog command args and payloads', async () => {
    const invoke = vi.fn().mockResolvedValue({
      sources: [
        {
          description: 'Unknown source.',
          id: 'bad source',
          installable: true,
          label: '',
          trustLevel: 'unknown',
        },
      ],
    })
    const client = createInvokeCommandClient(invoke)

    await expect(listSkillCatalogEntries({ sourceId: '' as never }, client)).rejects.toThrow(
      TauriCommandPayloadError,
    )
    await expect(
      listSkillCatalogEntries({ limit: 0, sourceId: 'anthropic' }, client),
    ).rejects.toThrow(TauriCommandPayloadError)
    await expect(
      getSkillCatalogEntry({ entryId: '', sourceId: 'anthropic' }, client),
    ).rejects.toThrow(TauriCommandPayloadError)
    await expect(
      getSkillCatalogFile(
        {
          entryId: 'anthropic:frontend-design',
          path: '../SKILL.md',
          sourceId: 'anthropic',
        },
        client,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
    await expect(
      installSkillFromCatalog(
        { entryId: 'anthropic:frontend-design', sourceId: '' as never },
        client,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
    await expect(
      installSkillFromCatalog(
        {
          entryId: 'anthropic:frontend-design',
          operationId: '',
          sourceId: 'anthropic',
        },
        client,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
    await expect(listSkillCatalogSources(client)).rejects.toThrow(TauriCommandPayloadError)

    let tauriEventHandler: ((event: { payload: unknown }) => void) | undefined
    tauriListenSpy.mockImplementationOnce(async (_eventName, handler) => {
      tauriEventHandler = handler
      return vi.fn()
    })
    const progressEvents: unknown[] = []
    await listenSkillCatalogInstallProgress((progress) => {
      progressEvents.push(progress)
    }, client)
    expect(() =>
      tauriEventHandler?.({
        payload: {
          entryId: 'anthropic:frontend-design',
          operationId: 'catalog-install-001',
          percent: 101,
          sourceId: 'anthropic',
          stage: 'downloaded',
        },
      }),
    ).toThrow(TauriCommandPayloadError)
    expect(progressEvents).toEqual([])
  })
})

describe('agent orchestration contracts', () => {
  it('accepts all capability unavailable reason variants', () => {
    expect(
      parseAgentCapabilities({
        agentTeamsAvailable: false,
        agentTeamsEnabled: false,
        backgroundAgentsAvailable: false,
        backgroundAgentsEnabled: false,
        subagentsAvailable: false,
        subagentsEnabled: true,
        unavailableReasons: [
          { capability: 'subagents', type: 'notCompiled' },
          {
            capability: 'subagents',
            type: 'runtimeStoreUnavailable',
            message: 'open failed',
          },
          { capability: 'agentTeams', type: 'permissionRuntimeUnavailable' },
          {
            capability: 'agentTeams',
            type: 'invalidAgentProfiles',
            message: 'bad profile',
          },
          {
            message: 'supervisor missing',
            type: 'backgroundSupervisorUnavailable',
          },
          {
            capability: 'backgroundAgents',
            message: 'worktree unavailable',
            type: 'workspaceIsolationUnavailable',
          },
        ],
      }),
    ).toMatchObject({
      unavailableReasons: [
        { type: 'notCompiled' },
        { type: 'runtimeStoreUnavailable' },
        { type: 'permissionRuntimeUnavailable' },
        { type: 'invalidAgentProfiles' },
        { type: 'backgroundSupervisorUnavailable' },
        { type: 'workspaceIsolationUnavailable' },
      ],
    })
  })

  it('rejects unknown capability unavailable reason type', () => {
    expect(() =>
      parseAgentCapabilities({
        agentTeamsAvailable: false,
        agentTeamsEnabled: false,
        backgroundAgentsAvailable: false,
        backgroundAgentsEnabled: false,
        subagentsAvailable: false,
        subagentsEnabled: false,
        unavailableReasons: [{ capability: 'subagents', type: 'unknownReason' }],
      }),
    ).toThrow()
  })

  it('accepts valid tool policy with team config', () => {
    expect(
      parseAgentToolPolicy({
        agentTeam: 'allowed',
        backgroundAgents: 'allowed',
        maxConcurrentSubagents: 2,
        maxDepth: 2,
        maxTeamMembers: 4,
        subagents: 'allowed',
        teamConfig: {
          leadProfileId: 'lead',
          maxTurnsPerGoal: 4,
          memberProfileIds: ['worker_a'],
          sharedMemoryPolicy: 'summaries_only',
          topology: 'coordinator_worker',
        },
        workspaceIsolation: 'git_worktree',
      }),
    ).toMatchObject({
      backgroundAgents: 'allowed',
      workspaceIsolation: 'git_worktree',
    })
  })

  it('rejects unknown isolation mode', () => {
    expect(() =>
      parseAgentToolPolicy({
        agentTeam: 'off',
        backgroundAgents: 'off',
        maxConcurrentSubagents: 1,
        maxDepth: 1,
        maxTeamMembers: 2,
        subagents: 'off',
        workspaceIsolation: 'shared_checkout',
      }),
    ).toThrow()
  })

  it('rejects unknown team topology', () => {
    expect(() =>
      parseAgentToolPolicy({
        agentTeam: 'allowed',
        backgroundAgents: 'off',
        maxConcurrentSubagents: 1,
        maxDepth: 1,
        maxTeamMembers: 2,
        subagents: 'allowed',
        teamConfig: {
          leadProfileId: 'lead',
          maxTurnsPerGoal: 1,
          memberProfileIds: ['worker_a'],
          sharedMemoryPolicy: 'none',
          topology: 'custom_mesh',
        },
        workspaceIsolation: 'read_only',
      }),
    ).toThrow()
  })

  it('rejects invalid profile id', () => {
    expect(() =>
      parseAgentProfile({
        contextMode: 'minimal',
        defaultWorkspaceIsolation: 'read_only',
        description: 'bad id',
        id: 'Invalid-ID',
        maxDepth: 1,
        maxTurns: 1,
        memoryScope: 'none',
        role: 'Worker',
        sandboxInheritance: 'inherit_parent',
        scope: 'user',
        toolBlocklist: [],
      }),
    ).toThrow()
  })

  it('rejects empty team member list', () => {
    expect(() =>
      parseAgentToolPolicy({
        agentTeam: 'allowed',
        backgroundAgents: 'off',
        maxConcurrentSubagents: 1,
        maxDepth: 1,
        maxTeamMembers: 2,
        subagents: 'allowed',
        teamConfig: {
          leadProfileId: 'lead',
          maxTurnsPerGoal: 1,
          memberProfileIds: [],
          sharedMemoryPolicy: 'none',
          topology: 'peer_to_peer',
        },
        workspaceIsolation: 'read_only',
      }),
    ).toThrow()
  })

  it('rejects negative concurrency values', () => {
    expect(() =>
      parseAgentToolPolicy({
        agentTeam: 'off',
        backgroundAgents: 'off',
        maxConcurrentSubagents: 0,
        maxDepth: 1,
        maxTeamMembers: 2,
        subagents: 'off',
        workspaceIsolation: 'read_only',
      }),
    ).toThrow()
  })

  it('accepts team allowed without eager team config', () => {
    const parsed = parseAgentToolPolicy({
      agentTeam: 'allowed',
      backgroundAgents: 'off',
      maxConcurrentSubagents: 1,
      maxDepth: 1,
      maxTeamMembers: 2,
      subagents: 'allowed',
      workspaceIsolation: 'read_only',
    })

    expect(parsed).toMatchObject({ agentTeam: 'allowed' })
    expect(parsed).not.toHaveProperty('teamConfig')
  })

  it('rejects invalid background agent tool policy string', () => {
    expect(() =>
      parseAgentToolPolicy({
        agentTeam: 'off',
        backgroundAgents: 'detached',
        maxConcurrentSubagents: 1,
        maxDepth: 1,
        maxTeamMembers: 2,
        subagents: 'off',
        workspaceIsolation: 'read_only',
      }),
    ).toThrow()
  })

  it('rejects unknown profile scope', () => {
    expect(() =>
      parseAgentProfile({
        contextMode: 'minimal',
        defaultWorkspaceIsolation: 'read_only',
        description: 'scope',
        id: 'worker',
        maxDepth: 1,
        maxTurns: 1,
        memoryScope: 'none',
        role: 'Worker',
        sandboxInheritance: 'inherit_parent',
        scope: 'workspace',
        toolBlocklist: [],
      }),
    ).toThrow()
  })

  it('normalizes list agent profiles IPC payloads', async () => {
    const invoke = vi.fn().mockResolvedValue({
      profiles: [
        {
          contextMode: 'focused',
          defaultWorkspaceIsolation: 'read_only',
          description: 'Read-only review subagent',
          id: 'reviewer',
          maxDepth: 1,
          maxTurns: 8,
          memoryScope: 'read_only',
          modelConfigOverride: {
            modelId: null,
            providerConfigId: null,
          },
          role: 'Reviewer',
          sandboxInheritance: 'narrow_only',
          scope: 'builtin',
          toolAllowlist: null,
          toolBlocklist: ['bash', 'write'],
        },
      ],
    })
    const client = createInvokeCommandClient(invoke)

    await expect(listAgentProfiles(client)).resolves.toMatchObject({
      profiles: [{ id: 'reviewer', scope: 'builtin' }],
    })
    expect(invoke).toHaveBeenCalledWith('list_agent_profiles')
  })

  it('accepts projected team activity segments without raw inter-agent messages', async () => {
    const page = validWorktreePage()
    const assistant = page.turns[0].assistant
    if (!assistant) {
      throw new Error('assistant fixture missing')
    }
    assistant.segments.push({
      kind: 'agentActivity',
      id: 'segment:agent-team:team-001',
      order: 4,
      activityKind: 'agentTeam',
      agentId: 'team-001',
      role: 'Migration team',
      taskSummary: 'Coordinate the migration',
      status: 'completed',
      resultSummary: 'Team completed.',
      team: {
        topology: 'coordinator_worker',
        lead: {
          agentId: 'agent-lead',
          role: 'Lead',
          status: 'completed',
        },
        members: [
          {
            agentId: 'agent-lead',
            role: 'Lead',
            status: 'completed',
          },
          {
            agentId: 'agent-worker',
            role: 'Worker',
            status: 'completed',
          },
        ],
        currentTasks: [
          {
            id: 'task-1',
            title: 'Audit composer payload',
            status: 'completed',
            assigneeProfileId: 'lead',
          },
        ],
        mailboxCount: 1,
        mailboxSummaries: ['Routed message message-1 to 1 member.'],
      },
      eventRefs: [{ eventId: 'event-009', cursor: cursor('', 9) }],
    })
    const client = createInvokeCommandClient(vi.fn().mockResolvedValue(page))

    await expect(
      pageConversationWorktree({ conversationId: 'conversation-001' }, client),
    ).resolves.toMatchObject({
      turns: [
        {
          assistant: {
            segments: [
              {},
              {},
              {},
              {},
              {
                team: {
                  mailboxSummaries: ['Routed message message-1 to 1 member.'],
                },
              },
            ],
          },
        },
      ],
    })
  })

  it('rejects projected team activity segments that expose raw inter-agent messages', async () => {
    const page = clone(validWorktreePage()) as unknown as {
      turns: Array<{
        assistant?: {
          segments: unknown[]
        }
      }>
    }
    const assistant = page.turns[0]?.assistant
    if (!assistant) {
      throw new Error('assistant fixture missing')
    }
    assistant.segments.push({
      kind: 'agentActivity',
      id: 'segment:agent-team:team-001',
      order: 4,
      activityKind: 'agentTeam',
      agentId: 'team-001',
      role: 'Migration team',
      taskSummary: 'Coordinate the migration',
      status: 'running',
      team: {
        topology: 'coordinator_worker',
        members: [],
        currentTasks: [],
        mailboxCount: 1,
        mailboxSummaries: ['Safe summary only'],
        rawMessages: ['secret raw payload'],
      },
    })
    const client = createInvokeCommandClient(vi.fn().mockResolvedValue(page))

    await expect(
      pageConversationWorktree({ conversationId: 'conversation-001' }, client),
    ).rejects.toBeInstanceOf(TauriCommandPayloadError)
  })

  it('validates save agent profile payloads before invoke', async () => {
    const invoke = vi.fn()
    const client = createInvokeCommandClient(invoke)

    await expect(
      saveAgentProfile(
        {
          contextMode: 'minimal',
          defaultWorkspaceIsolation: 'read_only',
          description: 'bad id',
          id: 'Invalid-ID',
          maxDepth: 1,
          maxTurns: 1,
          memoryScope: 'none',
          role: 'Worker',
          sandboxInheritance: 'inherit_parent',
          scope: 'user',
          toolBlocklist: [],
        },
        client,
      ),
    ).rejects.toBeInstanceOf(TauriCommandPayloadError)
    expect(invoke).not.toHaveBeenCalled()
  })

  it('validates delete agent profile payloads before invoke', async () => {
    const invoke = vi.fn()
    const client = createInvokeCommandClient(invoke)
    const invalidId: DeleteAgentProfileRequest['id'] = 'Invalid-ID'

    await expect(deleteAgentProfile(invalidId, client)).rejects.toBeInstanceOf(
      TauriCommandPayloadError,
    )
    expect(invoke).not.toHaveBeenCalled()
  })

  it('accepts start run requests without agentOptions', async () => {
    const invoke = vi.fn().mockResolvedValue({
      runId: 'run-001',
      status: 'started',
    })
    const client = createInvokeCommandClient(invoke)

    await expect(
      startRun(
        {
          conversationId: 'conversation-001',
          modelConfigId: 'provider-config-001',
          prompt: 'Run',
        },
        client,
      ),
    ).resolves.toMatchObject({
      runId: 'run-001',
    })
    expect(invoke).toHaveBeenCalledWith('start_run', {
      conversationId: 'conversation-001',
      modelConfigId: 'provider-config-001',
      prompt: 'Run',
    })
  })

  it('accepts background agent command payloads and responses', async () => {
    const runningAgent = {
      backgroundAgentId: 'bg-agent-001',
      conversationId: 'conversation-001',
      createdAt: '2026-06-30T00:00:00.000Z',
      parentRunId: 'run-001',
      pendingInputRequestId: 'request-001',
      pendingPermissionRequestId: 'permission-request-001',
      state: 'running',
      title: 'Run checks',
      updatedAt: '2026-06-30T00:01:00.000Z',
    } as const
    const invoke = vi.fn(async (command: string) => {
      switch (command) {
        case 'list_background_agents':
          return { agents: [runningAgent] }
        case 'get_background_agent':
          return { agent: runningAgent }
        case 'pause_background_agent':
          return { agent: { ...runningAgent, state: 'paused' } }
        case 'resume_background_agent':
          return { agent: { ...runningAgent, state: 'running' } }
        case 'cancel_background_agent':
          return { agent: { ...runningAgent, state: 'cancelled' } }
        case 'send_background_agent_input':
          return { agent: { ...runningAgent, state: 'running' } }
        case 'archive_background_agent':
          return { agent: { ...runningAgent, state: 'archived' } }
        case 'delete_background_agent':
          return { backgroundAgentId: 'bg-agent-001', status: 'deleted' }
        default:
          throw new Error(`unexpected command ${command}`)
      }
    })
    const client = createInvokeCommandClient(invoke)

    await expect(
      listBackgroundAgents({ conversationId: 'conversation-001', includeArchived: true }, client),
    ).resolves.toEqual({ agents: [runningAgent] })
    await expect(
      getBackgroundAgent({ backgroundAgentId: 'bg-agent-001' }, client),
    ).resolves.toEqual({ agent: runningAgent })
    await expect(
      pauseBackgroundAgent({ backgroundAgentId: 'bg-agent-001' }, client),
    ).resolves.toMatchObject({ agent: { state: 'paused' } })
    await expect(
      resumeBackgroundAgent({ backgroundAgentId: 'bg-agent-001' }, client),
    ).resolves.toMatchObject({ agent: { state: 'running' } })
    await expect(
      sendBackgroundAgentInput(
        {
          backgroundAgentId: 'bg-agent-001',
          input: 'Continue',
          requestId: 'request-001',
        },
        client,
      ),
    ).resolves.toMatchObject({ agent: { state: 'running' } })
    await expect(
      cancelBackgroundAgent({ backgroundAgentId: 'bg-agent-001' }, client),
    ).resolves.toMatchObject({ agent: { state: 'cancelled' } })
    await expect(
      archiveBackgroundAgent({ backgroundAgentId: 'bg-agent-001' }, client),
    ).resolves.toMatchObject({ agent: { state: 'archived' } })
    await expect(
      deleteBackgroundAgent({ backgroundAgentId: 'bg-agent-001' }, client),
    ).resolves.toEqual({
      backgroundAgentId: 'bg-agent-001',
      status: 'deleted',
    })

    expect(invoke).toHaveBeenCalledWith('list_background_agents', {
      conversationId: 'conversation-001',
      includeArchived: true,
    })
    expect(invoke).not.toHaveBeenCalledWith('start_background_agent', expect.anything())
  })

  it('rejects invalid background agent command payloads', async () => {
    const client = createInvokeCommandClient(vi.fn())

    await expect(
      pauseBackgroundAgent({ backgroundAgentId: '', conversationId: 'conversation-001' }, client),
    ).rejects.toThrow(TauriCommandPayloadError)
    await expect(
      sendBackgroundAgentInput(
        {
          backgroundAgentId: 'bg-agent-001',
          input: '',
          requestId: 'request-001',
        },
        client,
      ),
    ).rejects.toThrow(TauriCommandPayloadError)
  })

  it('rejects start run agentOptions at the desktop IPC boundary', async () => {
    const client = createInvokeCommandClient(vi.fn())

    await expect(
      startRun(
        {
          agentOptions: {
            agentTeam: 'off',
            backgroundAgents: 'allowed',
            maxConcurrentSubagents: 2,
            maxDepth: 2,
            maxTeamMembers: 2,
            subagents: 'allowed',
            teamConfig: null,
            workspaceIsolation: 'read_only',
          },
          conversationId: 'conversation-001',
          modelConfigId: 'provider-config-001',
          prompt: 'Run',
        } as unknown as StartRunRequest,
        client,
      ),
    ).rejects.toBeInstanceOf(TauriCommandPayloadError)
  })
})
