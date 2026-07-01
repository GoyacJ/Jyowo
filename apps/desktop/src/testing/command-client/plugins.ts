import type {
  GetPluginDetailResponse,
  ListPluginsResponse,
  PluginInstallReport,
  PluginOperationResult,
} from '@/shared/tauri/commands'

export const fixtureListPlugins: ListPluginsResponse = {
  allowProjectPlugins: false,
  plugins: [
    {
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
    },
  ],
}

export const fixturePluginInstallReport: PluginInstallReport = {
  sourcePath: '/tmp/formatter-plugin',
  valid: true,
  summary: fixtureListPlugins.plugins[0],
  warnings: [],
}

export const fixturePluginDetail: GetPluginDetailResponse = {
  plugin: {
    summary: fixtureListPlugins.plugins[0],
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
        lineWidth: {
          type: 'number',
        },
        formatOnSave: {
          type: 'boolean',
        },
        apiToken: {
          type: 'string',
          secret: true,
        },
      },
    },
    config: {
      lineWidth: 100,
      formatOnSave: true,
    },
    registeredCapabilities: fixtureListPlugins.plugins[0].capabilities,
    recentEvents: ['loaded'],
  },
}

export const fixturePluginOperation: PluginOperationResult = {
  pluginId: fixtureListPlugins.plugins[0].id,
  status: 'installed',
  summary: fixtureListPlugins.plugins[0],
  report: fixturePluginInstallReport,
}
