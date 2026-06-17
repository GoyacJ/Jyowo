import type { AppInfo, CommandClient, HarnessHealthcheck } from './commands'

const mockAppInfo: AppInfo = {
  name: 'Jyowo',
  version: '0.1.0',
  shell: 'tauri2-react',
  harness: {
    sdkCrate: 'jyowo_harness_sdk',
    mode: 'in-process',
  },
}

const mockHarnessHealthcheck: HarnessHealthcheck = {
  status: 'available',
  sdkCrate: 'jyowo_harness_sdk',
}

export interface MockCommandClientOptions {
  appInfo?: AppInfo
  healthcheck?: HarnessHealthcheck
  delayMs?: number
}

function wait(delayMs: number | undefined) {
  if (!delayMs) {
    return Promise.resolve()
  }

  return new Promise<void>((resolve) => {
    window.setTimeout(resolve, delayMs)
  })
}

export function createMockCommandClient(options: MockCommandClientOptions = {}): CommandClient {
  return {
    async getAppInfo() {
      await wait(options.delayMs)
      return options.appInfo ?? mockAppInfo
    },
    async getHarnessHealthcheck() {
      await wait(options.delayMs)
      return options.healthcheck ?? mockHarnessHealthcheck
    },
  }
}

export function createRejectedCommandClient(error: Error): CommandClient {
  return {
    getAppInfo: () => Promise.reject(error),
    getHarnessHealthcheck: () => Promise.reject(error),
  }
}
