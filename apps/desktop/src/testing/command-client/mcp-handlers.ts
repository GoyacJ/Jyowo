import type {
  ClearMcpDiagnosticsResponse,
  SaveBrowserMcpPresetResponse,
  SetMcpServerEnabledResponse,
  SubscribeMcpDiagnosticsResponse,
  UnsubscribeMcpDiagnosticsResponse,
} from '@/shared/tauri/commands'

import { wait } from './base'
import {
  fixtureListBrowserMcpPresets,
  fixtureListMcpDiagnostics,
  fixtureListMcpServers,
  fixtureMcpServerConfig,
  fixtureSaveBrowserMcpPreset,
  fixtureSaveMcpServer,
} from './mcp'
import type { TestCommandClientState, TestCommandHandlers } from './state'

type McpCommandKeys =
  | 'clearMcpDiagnostics'
  | 'deleteMcpServer'
  | 'getMcpServerConfig'
  | 'listBrowserMcpPresets'
  | 'listMcpDiagnostics'
  | 'listMcpServers'
  | 'listenMcpDiagnosticBatches'
  | 'restartMcpServer'
  | 'saveBrowserMcpPreset'
  | 'saveMcpServer'
  | 'setMcpServerEnabled'
  | 'subscribeMcpDiagnostics'
  | 'unsubscribeMcpDiagnostics'

export function createMcpCommandHandlers(
  state: TestCommandClientState,
): TestCommandHandlers<McpCommandKeys> {
  return {
    async clearMcpDiagnostics() {
      await wait(state.options.delayMs)
      return { status: 'cleared' } satisfies ClearMcpDiagnosticsResponse
    },
    async deleteMcpServer(configLayer, id) {
      await wait(state.options.delayMs)
      return { configLayer, id, status: 'deleted' }
    },
    async getMcpServerConfig(configLayer, id) {
      await wait(state.options.delayMs)
      if (
        state.options.mcpServerConfig?.server.configLayer === configLayer &&
        state.options.mcpServerConfig.server.id === id
      ) {
        return state.options.mcpServerConfig
      }
      if (
        fixtureMcpServerConfig.server.configLayer === configLayer &&
        fixtureMcpServerConfig.server.id === id
      ) {
        return fixtureMcpServerConfig
      }
      throw new Error(`MCP server not found: ${id}`)
    },
    async listBrowserMcpPresets() {
      await wait(state.options.delayMs)
      return state.options.browserMcpPresets ?? fixtureListBrowserMcpPresets
    },
    async listMcpDiagnostics() {
      await wait(state.options.delayMs)
      return state.options.mcpDiagnostics ?? fixtureListMcpDiagnostics
    },
    async listMcpServers(configLayer) {
      await wait(state.options.delayMs)
      const response = state.options.mcpServers ?? fixtureListMcpServers
      return response.configLayer === configLayer ? response : { configLayer, servers: [] }
    },
    async listenMcpDiagnosticBatches() {
      await wait(state.options.delayMs)
      return () => undefined
    },
    async restartMcpServer(configLayer, id) {
      await wait(state.options.delayMs)
      const server =
        (state.options.mcpServers ?? fixtureListMcpServers).servers.find(
          (server) => server.configLayer === configLayer && server.id === id,
        ) ?? fixtureSaveMcpServer.server
      return {
        server,
      }
    },
    async saveBrowserMcpPreset(request) {
      await wait(state.options.delayMs)
      const preset =
        (state.options.browserMcpPresets ?? fixtureListBrowserMcpPresets).presets.find(
          (preset) => preset.id === request.presetId,
        ) ?? fixtureListBrowserMcpPresets.presets[0]
      return (state.options.browserMcpPreset ?? {
        preset: {
          ...preset,
          enabled: request.enabled ?? false,
        },
        server: {
          ...fixtureSaveBrowserMcpPreset.server,
          displayName: preset.displayName,
          enabled: request.enabled ?? false,
          id: preset.serverId,
          status: request.enabled ? 'configured' : 'disabled',
        },
      }) satisfies SaveBrowserMcpPresetResponse
    },
    async saveMcpServer() {
      await wait(state.options.delayMs)
      return state.options.mcpServer ?? fixtureSaveMcpServer
    },
    async setMcpServerEnabled(configLayer, id, enabled) {
      await wait(state.options.delayMs)
      const server =
        (state.options.mcpServers ?? fixtureListMcpServers).servers.find(
          (server) => server.configLayer === configLayer && server.id === id,
        ) ?? fixtureSaveMcpServer.server
      return {
        server: {
          ...server,
          enabled,
          status: enabled ? server.status : 'disabled',
        },
      } satisfies SetMcpServerEnabledResponse
    },
    async subscribeMcpDiagnostics() {
      await wait(state.options.delayMs)
      return (state.options.subscribeMcpDiagnostics ?? {
        replayEvents: (state.options.mcpDiagnostics ?? fixtureListMcpDiagnostics).events,
        subscriptionId: 'mcp-diagnostic-subscription-001',
      }) satisfies SubscribeMcpDiagnosticsResponse
    },
    async unsubscribeMcpDiagnostics(subscriptionId) {
      await wait(state.options.delayMs)
      return {
        status: 'unsubscribed',
        subscriptionId,
      } satisfies UnsubscribeMcpDiagnosticsResponse
    },
  }
}
