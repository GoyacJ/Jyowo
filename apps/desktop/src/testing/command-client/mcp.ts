import type {
  GetMcpServerConfigResponse,
  ListBrowserMcpPresetsResponse,
  ListMcpDiagnosticsResponse,
  ListMcpServersResponse,
  SaveBrowserMcpPresetResponse,
  SaveMcpServerResponse,
} from '@/shared/tauri/commands'

import { timestamp } from './base'

export const fixtureListMcpServers: ListMcpServersResponse = {
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
  ],
}

export const fixtureListBrowserMcpPresets: ListBrowserMcpPresetsResponse = {
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

export const fixtureMcpServerConfig: GetMcpServerConfigResponse = {
  server: {
    displayName: 'Workspace GitHub',
    enabled: true,
    id: 'github',
    scope: 'global',
    transport: {
      args: ['mcp-server'],
      command: 'node',
      env: [{ hasValue: true, key: 'LOG_LEVEL' }],
      inheritEnv: ['GITHUB_TOKEN'],
      kind: 'stdio',
    },
  },
}

export const fixtureSaveMcpServer: SaveMcpServerResponse = {
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

export const fixtureSaveBrowserMcpPreset: SaveBrowserMcpPresetResponse = {
  preset: fixtureListBrowserMcpPresets.presets[0],
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

export const fixtureListMcpDiagnostics: ListMcpDiagnosticsResponse = {
  events: [
    {
      eventType: 'connection_recovered',
      id: 'mcp-diagnostic-001',
      serverId: 'github',
      severity: 'info',
      summary: 'MCP server connection recovered.',
      timestamp,
    },
  ],
}
