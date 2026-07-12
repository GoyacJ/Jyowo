import {
  cloneResponse,
  fixtureAppInfo,
  fixtureRuntimeExecutionStatus,
  fixtureRuntimeTools,
  wait,
} from './base'
import type { TestCommandClientState, TestCommandHandlers } from './state'

type BaseCommandKeys = 'getAppInfo' | 'getRuntimeExecutionStatus' | 'listRuntimeTools'

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
      return cloneResponse(state.options.runtimeTools ?? fixtureRuntimeTools)
    },
  }
}
