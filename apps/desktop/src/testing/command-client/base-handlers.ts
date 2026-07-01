import { fixtureAppInfo, fixtureHarnessHealthcheck, wait } from './base'
import type { TestCommandClientState, TestCommandHandlers } from './state'

type BaseCommandKeys = 'getAppInfo' | 'getHarnessHealthcheck'

export function createBaseCommandHandlers(
  state: TestCommandClientState,
): TestCommandHandlers<BaseCommandKeys> {
  return {
    async getAppInfo() {
      await wait(state.options.delayMs)
      return state.options.appInfo ?? fixtureAppInfo
    },
    async getHarnessHealthcheck() {
      await wait(state.options.delayMs)
      return state.options.healthcheck ?? fixtureHarnessHealthcheck
    },
  }
}
