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
      origin: 'user',
      overridesGlobal: false,
      required: false,
      scope: 'global',
      status: 'ready',
      statusSource: 'settings',
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

export const fixtureSaveMcpServer: SaveMcpServerResponse = {
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
}

export const fixtureSaveBrowserMcpPreset: SaveBrowserMcpPresetResponse = {
  preset: fixtureListBrowserMcpPresets.presets[0],
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

export const fixtureListMcpDiagnostics: ListMcpDiagnosticsResponse = {
  events: [
    {
      eventType: 'connection_recovered',
      id: 'mcp-diagnostic-001',
      plane: 'settings',
      serverId: 'github',
      severity: 'info',
      summary: 'MCP server connection recovered.',
      timestamp,
    },
  ],
}
