import type { PluginOperationResult } from '@/shared/tauri/commands'

import { wait } from './base'
import {
  fixtureListPlugins,
  fixturePluginDetail,
  fixturePluginInstallReport,
  fixturePluginOperation,
} from './plugins'
import type { TestCommandClientState, TestCommandHandlers } from './state'

type PluginCommandKeys =
  | 'getPluginDetail'
  | 'installPluginFromPath'
  | 'listPlugins'
  | 'reloadPlugin'
  | 'setPluginEnabled'
  | 'setProjectPluginsEnabled'
  | 'uninstallPlugin'
  | 'updatePluginConfig'
  | 'validatePluginFromPath'

export function createPluginCommandHandlers(
  state: TestCommandClientState,
): TestCommandHandlers<PluginCommandKeys> {
  return {
    async getPluginDetail(pluginId) {
      await wait(state.options.delayMs)
      if (state.options.pluginDetail?.plugin.summary.id === pluginId) {
        return state.options.pluginDetail
      }
      if (fixturePluginDetail.plugin.summary.id === pluginId) {
        return fixturePluginDetail
      }
      throw new Error(`Plugin not found: ${pluginId}`)
    },
    async installPluginFromPath() {
      await wait(state.options.delayMs)
      return state.options.pluginOperation ?? fixturePluginOperation
    },
    async listPlugins() {
      await wait(state.options.delayMs)
      return state.options.plugins ?? fixtureListPlugins
    },
    async reloadPlugin(pluginId) {
      await wait(state.options.delayMs)
      const summary =
        (state.options.plugins ?? fixtureListPlugins).plugins.find(
          (plugin) => plugin.id === pluginId,
        ) ?? fixtureListPlugins.plugins[0]
      return {
        pluginId,
        status: 'reloaded',
        summary,
      } satisfies PluginOperationResult
    },
    async setPluginEnabled(pluginId, enabled) {
      await wait(state.options.delayMs)
      const summary =
        (state.options.plugins ?? fixtureListPlugins).plugins.find(
          (plugin) => plugin.id === pluginId,
        ) ?? fixtureListPlugins.plugins[0]
      return {
        pluginId,
        status: enabled ? 'enabled' : 'disabled',
        summary: {
          ...summary,
          enabled,
          state: enabled ? 'activated' : { disabled: { last_state: 'activated' } },
        },
      } satisfies PluginOperationResult
    },
    async setProjectPluginsEnabled(enabled) {
      await wait(state.options.delayMs)
      return (
        state.options.setProjectPluginsEnabled ?? {
          allowProjectPlugins: enabled,
        }
      )
    },
    async uninstallPlugin(pluginId) {
      await wait(state.options.delayMs)
      return {
        pluginId,
        status: 'uninstalled',
      } satisfies PluginOperationResult
    },
    async updatePluginConfig(pluginId) {
      await wait(state.options.delayMs)
      const summary =
        (state.options.plugins ?? fixtureListPlugins).plugins.find(
          (plugin) => plugin.id === pluginId,
        ) ?? fixtureListPlugins.plugins[0]
      return {
        pluginId,
        status: 'configured',
        summary,
      } satisfies PluginOperationResult
    },
    async validatePluginFromPath() {
      await wait(state.options.delayMs)
      return state.options.pluginInstallReport ?? fixturePluginInstallReport
    },
  }
}
