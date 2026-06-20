import { type CommandClient, tauriCommandClient } from './commands'

interface CommandClientEnv {
  DEV: boolean
  VITE_JYOWO_COMMAND_CLIENT?: string
}

type RuntimeDetection = {
  hasTauriRuntime?: boolean
}

export function shouldUseMockCommandClient(env: CommandClientEnv, runtime: RuntimeDetection = {}) {
  if (!env.DEV || runtime.hasTauriRuntime === true) {
    return false
  }

  return env.VITE_JYOWO_COMMAND_CLIENT === 'mock'
}

export function createDefaultCommandClient() {
  if (import.meta.env.DEV) {
    const shouldUseMockClient = shouldUseMockCommandClient(import.meta.env, {
      hasTauriRuntime: hasTauriRuntime(),
    })

    if (shouldUseMockClient) {
      return createDeferredMockCommandClient()
    }
  }

  return tauriCommandClient
}

function createDeferredMockCommandClient(): CommandClient {
  let clientPromise: Promise<CommandClient> | undefined

  function getClient() {
    clientPromise ??= import('./mock-client').then(({ createMockCommandClient }) =>
      createMockCommandClient(),
    )

    return clientPromise
  }

  return {
    async cancelRun(runId) {
      return (await getClient()).cancelRun(runId)
    },
    async deleteMcpServer(id) {
      return (await getClient()).deleteMcpServer(id)
    },
    async deleteMemoryItem(id) {
      return (await getClient()).deleteMemoryItem(id)
    },
    async exportMemoryItems() {
      return (await getClient()).exportMemoryItems()
    },
    async exportSupportBundle(request) {
      return (await getClient()).exportSupportBundle(request)
    },
    async getAppInfo() {
      return (await getClient()).getAppInfo()
    },
    async getContextSnapshot(request) {
      return (await getClient()).getContextSnapshot(request)
    },
    async getConversation(conversationId) {
      return (await getClient()).getConversation(conversationId)
    },
    async getHarnessHealthcheck() {
      return (await getClient()).getHarnessHealthcheck()
    },
    async getMemoryItem(id) {
      return (await getClient()).getMemoryItem(id)
    },
    async getReplayTimeline(request) {
      return (await getClient()).getReplayTimeline(request)
    },
    async listActivity(request) {
      return (await getClient()).listActivity(request)
    },
    async listArtifacts() {
      return (await getClient()).listArtifacts()
    },
    async listConversations() {
      return (await getClient()).listConversations()
    },
    async listEvalCases() {
      return (await getClient()).listEvalCases()
    },
    async listMcpServers() {
      return (await getClient()).listMcpServers()
    },
    async listMemoryItems() {
      return (await getClient()).listMemoryItems()
    },
    async resolvePermission(request) {
      return (await getClient()).resolvePermission(request)
    },
    async runEvalCase(caseId) {
      return (await getClient()).runEvalCase(caseId)
    },
    async saveMcpServer(request) {
      return (await getClient()).saveMcpServer(request)
    },
    async saveProviderSettings(request) {
      return (await getClient()).saveProviderSettings(request)
    },
    async startRun(request) {
      return (await getClient()).startRun(request)
    },
    async updateMemoryItem(request) {
      return (await getClient()).updateMemoryItem(request)
    },
    async validateProviderSettings(request) {
      return (await getClient()).validateProviderSettings(request)
    },
  }
}

export function hasTauriRuntime() {
  if (typeof window === 'undefined') {
    return false
  }

  const tauriWindow = window as Window & {
    __TAURI__?: unknown
    __TAURI_INTERNALS__?: unknown
  }

  return Boolean(tauriWindow.__TAURI__ || tauriWindow.__TAURI_INTERNALS__)
}
