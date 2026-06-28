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
    async createAttachmentFromPath(path) {
      return (await getClient()).createAttachmentFromPath(path)
    },
    async deleteMcpServer(id) {
      return (await getClient()).deleteMcpServer(id)
    },
    async deleteMemoryItem(id) {
      return (await getClient()).deleteMemoryItem(id)
    },
    async deleteSkill(id) {
      return (await getClient()).deleteSkill(id)
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
    async getExecutionSettings() {
      return (await getClient()).getExecutionSettings()
    },
    async getConversation(conversationId) {
      return (await getClient()).getConversation(conversationId)
    },
    async getArtifactMediaPreview(request) {
      return (await getClient()).getArtifactMediaPreview(request)
    },
    async getHarnessHealthcheck() {
      return (await getClient()).getHarnessHealthcheck()
    },
    async getMemoryItem(id) {
      return (await getClient()).getMemoryItem(id)
    },
    async getMcpServerConfig(id) {
      return (await getClient()).getMcpServerConfig(id)
    },
    async getProviderConfigApiKey(configId, revealToken) {
      return (await getClient()).getProviderConfigApiKey(configId, revealToken)
    },
    async getReplayTimeline(request) {
      return (await getClient()).getReplayTimeline(request)
    },
    async getSkillCatalogEntry(request) {
      return (await getClient()).getSkillCatalogEntry(request)
    },
    async getSkillCatalogFile(request) {
      return (await getClient()).getSkillCatalogFile(request)
    },
    async pageConversationTimeline(request) {
      return (await getClient()).pageConversationTimeline(request)
    },
    async pageConversationWorktree(request) {
      return (await getClient()).pageConversationWorktree(request)
    },
    async getSkillDetail(id) {
      return (await getClient()).getSkillDetail(id)
    },
    async getSkillFile(id, path) {
      return (await getClient()).getSkillFile(id, path)
    },
    async importSkill(sourcePath) {
      return (await getClient()).importSkill(sourcePath)
    },
    async installSkillFromCatalog(request) {
      return (await getClient()).installSkillFromCatalog(request)
    },
    async listSkillCatalogInstallTasks() {
      return (await getClient()).listSkillCatalogInstallTasks()
    },
    async listenSkillCatalogInstallProgress(onProgress) {
      return (await getClient()).listenSkillCatalogInstallProgress(onProgress)
    },
    async listActivity(request) {
      return (await getClient()).listActivity(request)
    },
    async listArtifacts(request) {
      return (await getClient()).listArtifacts(request)
    },
    async listConversations() {
      return (await getClient()).listConversations()
    },
    async createConversation() {
      return (await getClient()).createConversation()
    },
    async deleteConversation(conversationId) {
      return (await getClient()).deleteConversation(conversationId)
    },
    async listEvalCases() {
      return (await getClient()).listEvalCases()
    },
    async listModelProviderCatalog() {
      return (await getClient()).listModelProviderCatalog()
    },
    async listMcpDiagnostics(serverId) {
      return (await getClient()).listMcpDiagnostics(serverId)
    },
    async listMcpServers() {
      return (await getClient()).listMcpServers()
    },
    async listMemoryItems() {
      return (await getClient()).listMemoryItems()
    },
    async listProviderSettings() {
      return (await getClient()).listProviderSettings()
    },
    async listProjects() {
      return (await getClient()).listProjects()
    },
    async addProject(path) {
      return (await getClient()).addProject(path)
    },
    async switchProject(path) {
      return (await getClient()).switchProject(path)
    },
    async deleteProject(path) {
      return (await getClient()).deleteProject(path)
    },
    async listReferenceCandidates(request) {
      return (await getClient()).listReferenceCandidates(request)
    },
    async listSkillCatalogEntries(request) {
      return (await getClient()).listSkillCatalogEntries(request)
    },
    async listSkillCatalogSources() {
      return (await getClient()).listSkillCatalogSources()
    },
    async listSkills() {
      return (await getClient()).listSkills()
    },
    async resolvePermission(request) {
      return (await getClient()).resolvePermission(request)
    },
    async requestProviderConfigApiKeyReveal(configId) {
      return (await getClient()).requestProviderConfigApiKeyReveal(configId)
    },
    async runEvalCase(caseId) {
      return (await getClient()).runEvalCase(caseId)
    },
    async saveMcpServer(request) {
      return (await getClient()).saveMcpServer(request)
    },
    async setMcpServerEnabled(id, enabled) {
      return (await getClient()).setMcpServerEnabled(id, enabled)
    },
    async restartMcpServer(id) {
      return (await getClient()).restartMcpServer(id)
    },
    async clearMcpDiagnostics(serverId) {
      return (await getClient()).clearMcpDiagnostics(serverId)
    },
    async saveProviderSettings(request) {
      return (await getClient()).saveProviderSettings(request)
    },
    async setExecutionSettings(request) {
      return (await getClient()).setExecutionSettings(request)
    },
    async setConversationModelConfig(conversationId, modelConfigId) {
      return (await getClient()).setConversationModelConfig(conversationId, modelConfigId)
    },
    async setSkillEnabled(id, enabled) {
      return (await getClient()).setSkillEnabled(id, enabled)
    },
    async startRun(request) {
      return (await getClient()).startRun(request)
    },
    async subscribeConversationEvents(request) {
      return (await getClient()).subscribeConversationEvents(request)
    },
    async listenConversationEventBatches(onBatch) {
      return (await getClient()).listenConversationEventBatches(onBatch)
    },
    async subscribeMcpDiagnostics(request) {
      return (await getClient()).subscribeMcpDiagnostics(request)
    },
    async listenMcpDiagnosticBatches(onBatch) {
      return (await getClient()).listenMcpDiagnosticBatches(onBatch)
    },
    async unsubscribeMcpDiagnostics(subscriptionId) {
      return (await getClient()).unsubscribeMcpDiagnostics(subscriptionId)
    },
    async unsubscribeConversationEvents(subscriptionId) {
      return (await getClient()).unsubscribeConversationEvents(subscriptionId)
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
