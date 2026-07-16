import {
  cloneResponse,
  fixtureAppInfo,
  fixtureRuntimeExecutionStatus,
  fixtureRuntimeTools,
  wait,
} from './base'
import type { TestCommandClientState, TestCommandHandlers } from './state'

type BaseCommandKeys =
  | 'getAppInfo'
  | 'getRuntimeExecutionStatus'
  | 'listRuntimeTools'
  | 'resetRuntimeTools'
  | 'resetRuntimeToolConfig'
  | 'setRuntimeToolEnabled'
  | 'updateRuntimeToolConfig'

export function createBaseCommandHandlers(
  state: TestCommandClientState,
): TestCommandHandlers<BaseCommandKeys> {
  return {
    async getAppInfo() {
      await wait(state.options.delayMs)
      return state.options.appInfo ?? fixtureAppInfo
    },
    async getRuntimeExecutionStatus() {
      await wait(state.options.delayMs)
      return cloneResponse(state.options.runtimeExecutionStatus ?? fixtureRuntimeExecutionStatus)
    },
    async listRuntimeTools() {
      await wait(state.options.delayMs)
      return cloneResponse(state.runtimeTools)
    },
    async setRuntimeToolEnabled(request) {
      await wait(state.options.delayMs)
      state.runtimeTools = {
        ...state.runtimeTools,
        customized: true,
        generation: state.runtimeTools.generation + 1,
        tools: state.runtimeTools.tools.map((tool) =>
          tool.name === request.name ? { ...tool, configuredEnabled: request.enabled } : tool,
        ),
      }
      return cloneResponse(state.runtimeTools)
    },
    async resetRuntimeTools() {
      await wait(state.options.delayMs)
      state.runtimeTools = {
        ...cloneResponse(state.options.runtimeTools ?? fixtureRuntimeTools),
        customized: false,
        generation: state.runtimeTools.generation + 1,
      }
      return cloneResponse(state.runtimeTools)
    },
    async updateRuntimeToolConfig(request) {
      await wait(state.options.delayMs)
      state.runtimeTools = {
        ...state.runtimeTools,
        customized: true,
        generation: state.runtimeTools.generation + 1,
        tools: state.runtimeTools.tools.map((tool) =>
          tool.name === request.name
            ? {
                ...tool,
                configurationCustomized: true,
                parameters: request.parameters,
                timeoutMs: request.timeoutMs,
              }
            : tool,
        ),
      }
      return cloneResponse(state.runtimeTools)
    },
    async resetRuntimeToolConfig(request) {
      await wait(state.options.delayMs)
      state.runtimeTools = {
        ...state.runtimeTools,
        generation: state.runtimeTools.generation + 1,
        tools: state.runtimeTools.tools.map((tool) =>
          tool.name === request.name
            ? {
                ...tool,
                configurationCustomized: false,
                parameters: tool.defaultParameters,
                timeoutMs: tool.defaultTimeoutMs,
              }
            : tool,
        ),
      }
      return cloneResponse(state.runtimeTools)
    },
  }
}
