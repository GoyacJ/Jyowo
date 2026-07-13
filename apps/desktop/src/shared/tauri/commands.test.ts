import { describe, expect, it, vi } from 'vitest'

const _validEvidenceContentHash = 'a'.repeat(64)
const tauriListenSpy = vi.hoisted(() => vi.fn())

vi.mock('@tauri-apps/api/event', () => ({
  listen: tauriListenSpy,
}))

import { createTestCommandClient } from '@/testing/command-client'
import {
  clearMcpDiagnostics,
  createInvokeCommandClient,
  type DeleteAgentProfileRequest,
  deleteAgentProfile,
  deleteMcpServer,
  deleteProject,
  deleteProviderCapabilityRoute,
  deleteSkill,
  getAppInfo,
  getDefaultWorkspace,
  getExecutionSettings,
  getMcpServerConfig,
  getModelSettingsPage,
  getModelUsageSummary,
  getPluginDetail,
  getProviderConfigApiKey,
  getSkillCatalogEntry,
  getSkillCatalogFile,
  getSkillDetail,
  getSkillFile,
  importSkill,
  installPluginFromPath,
  installSkillFromCatalog,
  listAgentProfiles,
  listBrowserMcpPresets,
  listenMcpDiagnosticBatches,
  listenSkillCatalogInstallProgress,
  listMcpDiagnostics,
  listMcpServers,
  listModelProviderCatalog,
  listOfficialQuotaSnapshots,
  listPlugins,
  listProviderCapabilityRouteOptions,
  listProviderCapabilityRoutes,
  listProviderProbeSnapshots,
  listProviderSettings,
  listSkillCatalogEntries,
  listSkillCatalogInstallTasks,
  listSkillCatalogSources,
  listSkills,
  moveProject,
  parseAgentCapabilities,
  parseAgentProfile,
  parseAgentToolPolicy,
  probeProviderConfig,
  refreshModelProviderCatalog,
  refreshOfficialQuota,
  reloadPlugin,
  renameProject,
  requestProviderConfigApiKeyReveal,
  restartMcpServer,
  saveAgentProfile,
  saveBrowserMcpPreset,
  saveMcpServer,
  saveProviderCapabilityRoute,
  saveProviderSettings,
  setExecutionSettings,
  setMcpServerEnabled,
  setPluginEnabled,
  setProjectPluginsEnabled,
  setSkillEnabled,
  subscribeMcpDiagnostics,
  TauriCommandPayloadError,
  type ToolProfile,
  uninstallPlugin,
  unsubscribeMcpDiagnostics,
  updatePluginConfig,
  validatePluginFromPath,
  validateProviderSettings,
} from './commands'
import { getCommandErrorMessage } from './errors'

const _validAttachmentId =
  'attachment-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'
const _validBlobRef = {
  contentHash: Array.from({ length: 32 }, () => 1),
  contentType: 'text/plain',
  id: '01J00000000000000000000000',
  size: 128,
}
const openAiModelDescriptor = {
  protocol: 'responses',
  supportedProtocols: ['responses'],
  supportedParameters: [],
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
const _openAiRunModelSnapshot = {
  modelConfigId: 'provider-config-001',
  providerId: 'openai',
  modelId: openAiModelDescriptor.modelId,
  displayName: openAiModelDescriptor.displayName,
  protocol: openAiModelDescriptor.protocol,
} as const

describe('CommandClient', () => {
  const _attachmentPreviewId =
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
      scope: 'global',
      toolProfile: 'coding',
    })
    const client = createInvokeCommandClient(invoke)

    await expect(getExecutionSettings(client)).resolves.toEqual({
      agentCapabilities,
      autoModeAvailable: false,
      contextCompressionTriggerRatio: 0.8,
      permissionMode: 'default',
      scope: 'global',
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
      scope: 'global',
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
    expect(getCommandErrorMessage('Command list_runtime_tools not found')).toBe(
      'Command list_runtime_tools not found',
    )
    expect(getCommandErrorMessage('')).toBe('Unknown command error')
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

  it('models the default workspace and project renaming through Zod validation', async () => {
    const invoke = vi
      .fn()
      .mockResolvedValueOnce({ path: '/Users/goya/.jyowo/workspaces/default' })
      .mockResolvedValueOnce({
        project: {
          path: '/Users/goya/Repo/Git/Jyowo',
          name: 'Jyowo Desktop',
          lastOpenedAt: '2026-07-12T00:00:00Z',
        },
      })
    const client = createInvokeCommandClient(invoke)

    await expect(getDefaultWorkspace(client)).resolves.toEqual({
      path: '/Users/goya/.jyowo/workspaces/default',
    })
    await expect(
      renameProject('/Users/goya/Repo/Git/Jyowo', 'Jyowo Desktop', client),
    ).resolves.toEqual({
      project: {
        path: '/Users/goya/Repo/Git/Jyowo',
        name: 'Jyowo Desktop',
        lastOpenedAt: '2026-07-12T00:00:00Z',
      },
    })
    expect(invoke).toHaveBeenNthCalledWith(1, 'get_default_workspace')
    expect(invoke).toHaveBeenNthCalledWith(2, 'rename_project', {
      name: 'Jyowo Desktop',
      path: '/Users/goya/Repo/Git/Jyowo',
    })
  })

  it('moves projects through Tauri with a direction', async () => {
    const invoke = vi.fn().mockResolvedValue({
      activePath: '/repo/alpha',
      projects: [
        {
          path: '/repo/beta',
          name: 'beta',
          lastOpenedAt: '2026-07-07T07:00:00.000Z',
        },
        {
          path: '/repo/alpha',
          name: 'alpha',
          lastOpenedAt: '2026-07-08T07:00:00.000Z',
        },
      ],
    })
    const client = createInvokeCommandClient(invoke)

    await expect(moveProject('/repo/beta', 'up', client)).resolves.toEqual({
      activePath: '/repo/alpha',
      projects: [
        {
          path: '/repo/beta',
          name: 'beta',
          lastOpenedAt: '2026-07-07T07:00:00.000Z',
        },
        {
          path: '/repo/alpha',
          name: 'alpha',
          lastOpenedAt: '2026-07-08T07:00:00.000Z',
        },
      ],
    })
    expect(invoke).toHaveBeenCalledWith('move_project', { direction: 'up', path: '/repo/beta' })
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
                  supportedProtocols: ['responses'],
                  supportedParameters: [],
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
        selectionScope: 'global',
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
              supportedProtocols: ['responses'],
              supportedParameters: [],
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
    await client.listProviderSettings('/workspace/project')
    expect(invoke).toHaveBeenCalledWith('list_model_provider_catalog')
    expect(invoke).toHaveBeenCalledWith('list_provider_settings')
    expect(invoke).toHaveBeenCalledWith('list_provider_settings', {
      workspaceRoot: '/workspace/project',
    })
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

  it('validates model settings page and explicit catalog refresh payloads', async () => {
    const catalog = {
      providers: [
        {
          defaultBaseUrl: 'https://api.openai.com',
          displayName: 'OpenAI',
          models: [openAiModelDescriptor],
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
    const usageTotal = {
      cacheReadTokens: 0,
      cacheWriteTokens: 0,
      costMicros: 0,
      inputTokens: 1,
      outputTokens: 2,
      toolCalls: 0,
    }
    const usageWindow = {
      period: 'today',
      periodStart: '2026-06-30T00:00:00Z',
      periodEnd: '2026-06-30T12:00:00Z',
      total: usageTotal,
      byModel: [],
    }
    const usageActivity = {
      rangeStart: '2025-07-01',
      rangeEnd: '2026-06-30',
      peakDayTokens: 3,
      currentStreakDays: 1,
      longestStreakDays: 2,
      longestTaskDurationMs: 120000,
      days: [
        {
          date: '2026-06-30',
          usage: usageTotal,
        },
      ],
    }
    const providerSettings = {
      defaultConfigId: 'openai',
      selectionScope: 'global',
      configs: [
        {
          displayName: 'OpenAI',
          hasApiKey: true,
          hasOfficialQuotaApiKey: false,
          id: 'openai',
          isDefault: true,
          modelDescriptor: openAiModelDescriptor,
          modelId: 'gpt-5.4-mini',
          protocol: 'responses',
          providerId: 'openai',
        },
      ],
    }
    const pagePayload = {
      catalog,
      catalogSnapshot: {
        source: 'snapshot',
        lastSuccessfulRefreshAt: '2026-06-30T12:00:00Z',
        lastAttemptAt: '2026-06-30T12:00:00Z',
      },
      providerSettings,
      probeSnapshots: { status: 'ready', data: { snapshots: [] } },
      usageSummary: {
        status: 'ready',
        data: {
          timezoneId: 'UTC',
          timezoneOffsetMinutes: 0,
          today: usageWindow,
          monthToDate: { ...usageWindow, period: 'month_to_date' },
          allTime: {
            ...usageWindow,
            period: 'all_time',
            periodStart: undefined,
            periodEnd: undefined,
          },
          activity: usageActivity,
          generatedAt: '2026-06-30T12:00:00Z',
        },
      },
      quotaSnapshots: { status: 'error', safeMessage: 'Quota unavailable' },
      capabilityRoutes: { status: 'ready', data: { version: 1, routes: [] } },
      capabilityRouteOptions: { status: 'rebuilding', safeMessage: 'Routes rebuilding' },
      generatedAt: '2026-06-30T12:00:00Z',
    }
    const invoke = vi.fn(async (command: string) => {
      if (command === 'refresh_model_provider_catalog') {
        return {
          catalog,
          catalogSnapshot: {
            source: 'snapshot',
            lastSuccessfulRefreshAt: '2026-06-30T12:00:00Z',
            lastAttemptAt: '2026-06-30T12:00:00Z',
          },
        }
      }
      return pagePayload
    })
    const client = createInvokeCommandClient(invoke)

    await expect(getModelSettingsPage(client)).resolves.toEqual(pagePayload)
    await expect(refreshModelProviderCatalog(client)).resolves.toHaveProperty(
      'catalogSnapshot.source',
      'snapshot',
    )
    expect(invoke).toHaveBeenCalledWith('get_model_settings_page')
    expect(invoke).toHaveBeenCalledWith('refresh_model_provider_catalog')
  })

  it('rejects provider configs without model descriptors', async () => {
    const invoke = vi.fn().mockResolvedValue({
      defaultConfigId: 'openai',
      selectionScope: 'global',
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
    const usageActivity = {
      rangeStart: '2025-07-01',
      rangeEnd: '2026-06-30',
      peakDayTokens: 18,
      currentStreakDays: 1,
      longestStreakDays: 3,
      longestTaskDurationMs: 45000,
      days: [{ date: '2026-06-30', usage: usageWindow.total }],
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
      activity: usageActivity,
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
      activity: usageActivity,
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
        activity: usageActivity,
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
          configLayer: 'global',
          servers: [
            {
              configLayer: 'global',
              displayName: 'Workspace GitHub',
              effective: true,
              enabled: true,
              exposedToolCount: 2,
              id: 'github',
              manageable: true,
              overridesGlobal: false,
              origin: 'workspace',
              required: false,
              scope: 'global',
              status: 'ready',
              statusSource: 'settings',
              transport: 'stdio',
            },
            {
              configLayer: 'global',
              displayName: 'Plugin Context',
              effective: true,
              enabled: true,
              exposedToolCount: 1,
              id: 'plugin-context',
              manageable: false,
              overridesGlobal: false,
              origin: 'plugin',
              required: false,
              scope: 'session',
              sourcePluginId: 'formatter@1.0.0',
              status: 'ready',
              statusSource: 'settings',
              transport: 'http',
            },
          ],
        }
      }

      if (command === 'get_mcp_server_config') {
        return {
          server: {
            configLayer: 'global',
            displayName: 'Workspace GitHub',
            effective: true,
            enabled: true,
            id: 'github',
            manageable: true,
            overridesGlobal: false,
            required: false,
            scope: 'global',
            transport: {
              args: ['mcp-server'],
              command: 'node',
              env: [{ hasValue: true, key: 'LOG_LEVEL' }],
              inheritEnv: ['PATH'],
              kind: 'stdio',
            },
          },
        }
      }

      if (command === 'delete_mcp_server') {
        return {
          configLayer: 'global',
          id: 'github',
          status: 'deleted',
        }
      }

      return {
        server: {
          configLayer: 'global',
          displayName: 'Workspace GitHub',
          effective: true,
          enabled: true,
          exposedToolCount: 0,
          id: 'github',
          manageable: true,
          overridesGlobal: false,
          origin: 'workspace',
          required: false,
          scope: 'global',
          status: 'configured',
          statusSource: 'settings',
          transport: 'stdio',
        },
      }
    })
    const client = createInvokeCommandClient(invoke)

    await expect(listMcpServers('global', client)).resolves.toEqual({
      configLayer: 'global',
      servers: [
        {
          configLayer: 'global',
          displayName: 'Workspace GitHub',
          effective: true,
          enabled: true,
          exposedToolCount: 2,
          id: 'github',
          manageable: true,
          overridesGlobal: false,
          origin: 'workspace',
          required: false,
          scope: 'global',
          status: 'ready',
          statusSource: 'settings',
          transport: 'stdio',
        },
        {
          configLayer: 'global',
          displayName: 'Plugin Context',
          effective: true,
          enabled: true,
          exposedToolCount: 1,
          id: 'plugin-context',
          manageable: false,
          overridesGlobal: false,
          origin: 'plugin',
          required: false,
          scope: 'session',
          sourcePluginId: 'formatter@1.0.0',
          status: 'ready',
          statusSource: 'settings',
          transport: 'http',
        },
      ],
    })
    await expect(getMcpServerConfig('global', 'github', client)).resolves.toEqual({
      server: {
        configLayer: 'global',
        displayName: 'Workspace GitHub',
        effective: true,
        enabled: true,
        id: 'github',
        manageable: true,
        overridesGlobal: false,
        required: false,
        scope: 'global',
        transport: {
          args: ['mcp-server'],
          command: 'node',
          env: [{ hasValue: true, key: 'LOG_LEVEL' }],
          inheritEnv: ['PATH'],
          kind: 'stdio',
        },
      },
    })
    await expect(
      saveMcpServer(
        {
          configLayer: 'global',
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
    await expect(deleteMcpServer('global', 'github', client)).resolves.toEqual({
      configLayer: 'global',
      id: 'github',
      status: 'deleted',
    })

    expect(JSON.stringify(invoke.mock.results)).not.toContain('Authorization')
    expect(invoke).toHaveBeenCalledWith('list_mcp_servers', { configLayer: 'global' })
    expect(invoke).toHaveBeenCalledWith('get_mcp_server_config', {
      configLayer: 'global',
      id: 'github',
    })
    expect(invoke).toHaveBeenCalledWith('save_mcp_server', {
      displayName: 'Workspace GitHub',
      configLayer: 'global',
      enabled: true,
      id: 'github',
      required: false,
      scope: 'global',
      transport: {
        args: ['mcp-server'],
        command: 'node',
        env: [],
        inheritEnv: [],
        kind: 'stdio',
      },
    })
    expect(invoke).toHaveBeenCalledWith('delete_mcp_server', {
      configLayer: 'global',
      id: 'github',
    })
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
              version: '0.0.78',
            },
            {
              description: 'Browser inspection through Chrome DevTools MCP.',
              displayName: 'Chrome DevTools Browser',
              enabled: false,
              id: 'chrome-devtools',
              serverId: 'browser-chrome-devtools',
              version: '1.5.0',
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
          version: '0.0.78',
        },
        server: {
          configLayer: 'global',
          displayName: 'Playwright Browser',
          effective: true,
          enabled: false,
          exposedToolCount: 0,
          id: 'browser-playwright',
          manageable: true,
          origin: 'user',
          overridesGlobal: false,
          required: false,
          scope: 'global',
          status: 'disabled',
          statusSource: 'settings',
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
          version: '0.0.78',
        },
        {
          description: 'Browser inspection through Chrome DevTools MCP.',
          displayName: 'Chrome DevTools Browser',
          enabled: false,
          id: 'chrome-devtools',
          serverId: 'browser-chrome-devtools',
          version: '1.5.0',
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
            configLayer: 'global',
            displayName: 'Remote Context',
            effective: true,
            enabled: true,
            exposedToolCount: 0,
            id: 'context7',
            manageable: true,
            origin: 'user',
            overridesGlobal: false,
            required: false,
            scope: 'global',
            status: 'configured',
            statusSource: 'settings',
            transport: 'http',
          },
        }
      }

      return {
        server: {
          configLayer: 'global',
          displayName: 'Workspace GitHub',
          effective: true,
          enabled: true,
          exposedToolCount: 1,
          id: 'github',
          manageable: true,
          origin: 'user',
          overridesGlobal: false,
          required: false,
          scope: 'global',
          status: 'ready',
          statusSource: 'settings',
          transport: 'stdio',
        },
      }
    })
    const client = createInvokeCommandClient(invoke)

    await expect(
      saveMcpServer(
        {
          configLayer: 'global',
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
    await expect(setMcpServerEnabled('global', 'github', true, client)).resolves.toHaveProperty(
      'server.status',
      'ready',
    )
    await expect(restartMcpServer('global', 'github', client)).resolves.toHaveProperty(
      'server.transport',
      'stdio',
    )

    expect(JSON.stringify(invoke.mock.calls)).not.toContain('mcp-secret-token')
    expect(invoke).toHaveBeenCalledWith('save_mcp_server', {
      displayName: 'Remote Context',
      configLayer: 'global',
      enabled: true,
      id: 'context7',
      required: false,
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
      configLayer: 'global',
      enabled: true,
      id: 'github',
    })
    expect(invoke).toHaveBeenCalledWith('restart_mcp_server', {
      configLayer: 'global',
      id: 'github',
    })
  })

  it('accepts read-only in-process MCP server configs', async () => {
    const invoke = vi.fn(async () => ({
      server: {
        configLayer: 'global',
        displayName: 'Plugin Context',
        effective: true,
        enabled: true,
        id: 'plugin-context',
        manageable: false,
        overridesGlobal: false,
        required: false,
        scope: 'session',
        transport: { kind: 'inProcess' },
      },
    }))
    const client = createInvokeCommandClient(invoke)

    await expect(getMcpServerConfig('global', 'plugin-context', client)).resolves.toHaveProperty(
      'server.transport.kind',
      'inProcess',
    )
  })

  it('accepts MCP save requests that preserve existing redacted inline values', async () => {
    const invoke = vi.fn(async () => ({
      server: {
        configLayer: 'global',
        displayName: 'Workspace GitHub',
        effective: true,
        enabled: true,
        exposedToolCount: 0,
        id: 'github',
        manageable: true,
        origin: 'user',
        overridesGlobal: false,
        required: false,
        scope: 'global',
        status: 'configured',
        statusSource: 'settings',
        transport: 'stdio',
      },
    }))
    const client = createInvokeCommandClient(invoke)

    await expect(
      saveMcpServer(
        {
          configLayer: 'global',
          displayName: 'Workspace GitHub',
          id: 'github',
          scope: 'global',
          transport: {
            command: 'node',
            env: [{ key: 'LOG_LEVEL', preserveExisting: true }],
            kind: 'stdio',
          },
        },
        client,
      ),
    ).resolves.toHaveProperty('server.status', 'configured')

    expect(invoke).toHaveBeenCalledWith('save_mcp_server', {
      displayName: 'Workspace GitHub',
      configLayer: 'global',
      enabled: true,
      id: 'github',
      required: false,
      scope: 'global',
      transport: {
        args: [],
        command: 'node',
        env: [{ key: 'LOG_LEVEL', preserveExisting: true }],
        inheritEnv: [],
        kind: 'stdio',
      },
    })
    expect(JSON.stringify(invoke.mock.calls)).not.toContain('info')
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
        configLayer: 'global',
        displayName: 'Remote Context',
        effective: true,
        enabled: true,
        id: 'context7',
        manageable: true,
        overridesGlobal: false,
        required: false,
        scope: 'global',
        transport: {
          headers: [
            {
              hasValue: true,
              key: 'Authorization',
              value: 'Bearer mcp-secret-token',
            },
          ],
          kind: 'http',
          url: 'https://mcp.example.com/mcp',
        },
      },
    }))
    const client = createInvokeCommandClient(invoke)

    await expect(getMcpServerConfig('global', 'context7', client)).rejects.toThrow(
      TauriCommandPayloadError,
    )
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
    await expect(deleteMcpServer('global', '', client)).rejects.toThrow(TauriCommandPayloadError)
    expect(invoke).not.toHaveBeenCalled()
  })

  it('addresses MCP settings by config layer and parses explicit source metadata', async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === 'list_mcp_servers') {
        return {
          configLayer: 'project',
          servers: [
            {
              configLayer: 'project',
              displayName: 'Project MCP',
              effective: true,
              enabled: true,
              exposedToolCount: 2,
              id: 'project-mcp',
              manageable: true,
              origin: 'project',
              overridesGlobal: true,
              required: true,
              scope: 'session',
              status: 'ready',
              statusSource: 'settings',
              transport: 'stdio',
            },
          ],
        }
      }
      if (command === 'list_mcp_diagnostics') {
        return {
          events: [
            {
              eventType: 'activation_failed',
              id: 'event-1',
              plane: 'task',
              runId: 'run-1',
              runSegmentId: 'segment-1',
              serverId: 'project-mcp',
              sessionId: 'session-1',
              severity: 'error',
              summary: 'MCP activation failed.',
              taskId: 'task-1',
              timestamp: '2026-07-13T00:00:00Z',
            },
          ],
        }
      }
      return {
        server: {
          configLayer: 'project',
          displayName: 'Project MCP',
          effective: true,
          enabled: false,
          exposedToolCount: 0,
          id: 'project-mcp',
          manageable: true,
          origin: 'project',
          overridesGlobal: false,
          required: true,
          scope: 'global',
          status: 'disabled',
          statusSource: 'settings',
          transport: 'stdio',
        },
      }
    })
    const client = createInvokeCommandClient(invoke)

    await expect(client.listMcpServers('project')).resolves.toHaveProperty(
      'servers.0.configLayer',
      'project',
    )
    await expect(
      client.saveMcpServer({
        configLayer: 'project',
        displayName: 'Project MCP',
        enabled: false,
        id: 'project-mcp',
        required: true,
        scope: 'global',
        transport: { command: 'node', kind: 'stdio' },
      }),
    ).resolves.toHaveProperty('server.required', true)
    await expect(client.listMcpDiagnostics()).resolves.toHaveProperty('events.0.plane', 'task')

    expect(invoke).toHaveBeenCalledWith('list_mcp_servers', { configLayer: 'project' })
    expect(invoke).toHaveBeenCalledWith(
      'save_mcp_server',
      expect.objectContaining({ configLayer: 'project', required: true }),
    )
  })

  it('rejects MCP values outside Rust persistence constraints before invoking Tauri', async () => {
    const invalidRequests = [
      {
        configLayer: 'global',
        displayName: 'x'.repeat(257),
        id: 'too-long-name',
        required: false,
        scope: 'global',
        transport: { command: 'node', kind: 'stdio' },
      },
      {
        configLayer: 'global',
        displayName: 'NUL argument',
        id: 'nul-argument',
        required: false,
        scope: 'global',
        transport: { args: ['bad\0argument'], command: 'node', kind: 'stdio' },
      },
      {
        configLayer: 'global',
        displayName: 'User info',
        id: 'url-user-info',
        required: false,
        scope: 'global',
        transport: { kind: 'http', url: 'https://user:pass@mcp.example.com/mcp' },
      },
      {
        configLayer: 'global',
        displayName: 'Secret query',
        id: 'secret-query',
        required: false,
        scope: 'global',
        transport: { kind: 'http', url: 'https://mcp.example.com/mcp?api_key=public' },
      },
      {
        configLayer: 'global',
        displayName: 'Secret inherit',
        id: 'secret-inherit',
        required: false,
        scope: 'global',
        transport: { command: 'node', inheritEnv: ['GITHUB_TOKEN'], kind: 'stdio' },
      },
      {
        configLayer: 'global',
        displayName: 'Bad header',
        id: 'bad-header',
        required: false,
        scope: 'global',
        transport: {
          headers: [{ key: 'Bad Header', value: 'value' }],
          kind: 'http',
          url: 'https://mcp.example.com/mcp',
        },
      },
      {
        configLayer: 'global',
        displayName: 'Bad header value',
        id: 'bad-header-value',
        required: false,
        scope: 'global',
        transport: {
          headers: [{ key: 'X-Value', value: 'first\nsecond' }],
          kind: 'http',
          url: 'https://mcp.example.com/mcp',
        },
      },
      {
        configLayer: 'global',
        displayName: 'Secret-bearing header value',
        id: 'secret-bearing-header-value',
        required: false,
        scope: 'global',
        transport: {
          headers: [{ key: 'X-Value', value: 'Bearer public-value' }],
          kind: 'http',
          url: 'https://mcp.example.com/mcp',
        },
      },
    ]
    const invoke = vi.fn()
    const client = createInvokeCommandClient(invoke)

    for (const request of invalidRequests) {
      await expect(client.saveMcpServer(request as never)).rejects.toThrow(TauriCommandPayloadError)
    }
    expect(invoke).not.toHaveBeenCalled()
  })

  it('rejects MCP fields that exceed Rust UTF-8 byte limits before invoking Tauri', async () => {
    const oversized4KiB = '界'.repeat(1366)
    const oversized8KiB = '界'.repeat(2731)
    const invalidRequests = [
      {
        configLayer: 'global',
        displayName: '界'.repeat(86),
        id: 'display-name-bytes',
        required: false,
        scope: 'global',
        transport: { command: 'node', kind: 'stdio' },
      },
      {
        configLayer: 'global',
        displayName: 'Command bytes',
        id: 'command-bytes',
        required: false,
        scope: 'global',
        transport: { command: oversized4KiB, kind: 'stdio' },
      },
      {
        configLayer: 'global',
        displayName: 'Argument bytes',
        id: 'argument-bytes',
        required: false,
        scope: 'global',
        transport: { args: [oversized4KiB], command: 'node', kind: 'stdio' },
      },
      {
        configLayer: 'global',
        displayName: 'Environment bytes',
        id: 'environment-bytes',
        required: false,
        scope: 'global',
        transport: {
          command: 'node',
          env: [{ key: 'LOG_LEVEL', value: oversized4KiB }],
          kind: 'stdio',
        },
      },
      {
        configLayer: 'global',
        displayName: 'Working directory bytes',
        id: 'working-directory-bytes',
        required: false,
        scope: 'global',
        transport: { command: 'node', kind: 'stdio', workingDir: oversized4KiB },
      },
      {
        configLayer: 'global',
        displayName: 'Header bytes',
        id: 'header-bytes',
        required: false,
        scope: 'global',
        transport: {
          headers: [{ key: 'X-Value', value: oversized8KiB }],
          kind: 'http',
          url: 'https://mcp.example.com/mcp',
        },
      },
    ]
    const invoke = vi.fn()
    const client = createInvokeCommandClient(invoke)

    for (const request of invalidRequests) {
      await expect(client.saveMcpServer(request as never)).rejects.toThrow(TauriCommandPayloadError)
    }
    expect(invoke).not.toHaveBeenCalled()
  })

  it('rejects every Rust-classified MCP secret form before invoking Tauri', async () => {
    const tokenLike = 'a'.repeat(32)
    const invalidRequests = [
      {
        configLayer: 'global',
        displayName: 'Hyphenated secret query key',
        id: 'hyphenated-query-key',
        required: false,
        scope: 'global',
        transport: { kind: 'http', url: 'https://mcp.example.com/mcp?api-key=public' },
      },
      {
        configLayer: 'global',
        displayName: 'Token-like environment value',
        id: 'token-like-env',
        required: false,
        scope: 'global',
        transport: {
          command: 'node',
          env: [{ key: 'LOG_LEVEL', value: tokenLike }],
          kind: 'stdio',
        },
      },
      {
        configLayer: 'global',
        displayName: 'Token-like header value',
        id: 'token-like-header',
        required: false,
        scope: 'global',
        transport: {
          headers: [{ key: 'X-Value', value: tokenLike }],
          kind: 'http',
          url: 'https://mcp.example.com/mcp',
        },
      },
      {
        configLayer: 'global',
        displayName: 'Token-like query value',
        id: 'token-like-query',
        required: false,
        scope: 'global',
        transport: { kind: 'http', url: `https://mcp.example.com/mcp?value=${tokenLike}` },
      },
      {
        configLayer: 'global',
        displayName: 'Known secret prefix',
        id: 'known-secret-prefix',
        required: false,
        scope: 'global',
        transport: {
          command: 'node',
          env: [{ key: 'LOG_LEVEL', value: 'ghp_short' }],
          kind: 'stdio',
        },
      },
    ]
    const invoke = vi.fn()
    const client = createInvokeCommandClient(invoke)

    for (const request of invalidRequests) {
      await expect(client.saveMcpServer(request as never)).rejects.toThrow(TauriCommandPayloadError)
    }
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
  it('accepts the daemon capability unavailable reason', () => {
    expect(
      parseAgentCapabilities({
        agentTeamsAvailable: false,
        agentTeamsEnabled: false,
        backgroundAgentsAvailable: false,
        backgroundAgentsEnabled: false,
        subagentsAvailable: false,
        subagentsEnabled: true,
        unavailableReasons: [
          {
            capability: 'subagents',
            type: 'daemonUnavailable',
            message: 'task daemon is unavailable',
          },
        ],
      }),
    ).toMatchObject({
      unavailableReasons: [{ type: 'daemonUnavailable' }],
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
})
