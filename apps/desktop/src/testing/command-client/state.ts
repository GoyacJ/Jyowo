import type {
  CommandClient,
  ConversationEventBatchPayload,
  GetConversationResponse,
  InstallSkillFromCatalogRequest,
  ListAutomationRunsResponse,
  ListAutomationsResponse,
  ListConversationsResponse,
  ListProjectsResponse,
  ListProviderCapabilityRouteOptionsResponse,
  ListProviderCapabilityRoutesResponse,
  ListProviderSettingsResponse,
  PageConversationWorktreeResponse,
  SkillCatalogInstallProgressPayload,
  SubscribeConversationEventsResponse,
} from '@/shared/tauri/commands'

import { cloneResponse, type TestCommandClientOptions } from './base'
import {
  type FixtureConversationEventState,
  fixtureConversation,
  fixtureConversationWorktreePage,
  fixtureListConversations,
} from './conversation'
import {
  fixtureAutomationRuns,
  fixtureListAutomations,
  fixtureProviderSettingsList,
  testJyowoProject,
} from './settings'

export type TestCommandHandlers<T extends keyof CommandClient> = Pick<CommandClient, T>

export type TestCommandClientState = {
  activeSubscription: SubscribeConversationEventsResponse | null
  automationRuns: ListAutomationRunsResponse
  automations: ListAutomationsResponse
  batchListener: ((batch: ConversationEventBatchPayload) => void) | null
  catalogInstallProgressListeners: Set<(progress: SkillCatalogInstallProgressPayload) => void>
  clearPendingBatches: () => void
  completionBatchFlushed: Promise<void>
  conversationDetailsById: Map<string, GetConversationResponse>
  conversations: ListConversationsResponse
  createdConversationCounter: number
  emitCatalogInstallProgress: (
    request: InstallSkillFromCatalogRequest,
    stage: SkillCatalogInstallProgressPayload['stage'],
    percent: number,
  ) => void
  fixtureEventState: FixtureConversationEventState
  options: TestCommandClientOptions
  pendingBatchTimeouts: Map<number, () => void>
  projects: ListProjectsResponse
  providerCapabilityRouteOptions: ListProviderCapabilityRouteOptionsResponse
  providerCapabilityRoutes: ListProviderCapabilityRoutesResponse
  providerRevealConfigIdsByToken: Map<string, string>
  providerRevealCounter: number
  providerSettings: ListProviderSettingsResponse
  subscriptionCounter: number
  worktreePagesByConversation: Map<string, PageConversationWorktreeResponse>
}

export function createTestCommandClientState(
  options: TestCommandClientOptions,
): TestCommandClientState {
  const conversationDetailsById = new Map<string, GetConversationResponse>()
  conversationDetailsById.set(
    'conversation-001',
    cloneResponse(options.conversation ?? fixtureConversation),
  )

  const worktreePagesByConversation = new Map<string, PageConversationWorktreeResponse>()
  worktreePagesByConversation.set(
    'conversation-001',
    cloneResponse(options.conversationWorktreePage ?? fixtureConversationWorktreePage),
  )

  const state: TestCommandClientState = {
    activeSubscription: null,
    automationRuns: cloneResponse(options.automationRuns ?? fixtureAutomationRuns),
    automations: cloneResponse(options.automations ?? fixtureListAutomations),
    batchListener: null,
    catalogInstallProgressListeners: new Set(),
    clearPendingBatches: () => undefined,
    completionBatchFlushed: Promise.resolve(),
    conversationDetailsById,
    conversations: cloneResponse(options.conversations ?? fixtureListConversations),
    createdConversationCounter: 0,
    emitCatalogInstallProgress: () => undefined,
    fixtureEventState: {
      getListener: () => null,
      getSubscription: () => null,
      trackTimeout: () => undefined,
      untrackTimeout: () => undefined,
    },
    options,
    pendingBatchTimeouts: new Map(),
    projects: options.projects ?? testJyowoProject,
    providerCapabilityRouteOptions: cloneResponse(
      options.providerCapabilityRouteOptions ?? {
        options: [],
      },
    ),
    providerCapabilityRoutes: cloneResponse(
      options.providerCapabilityRoutes ?? {
        version: 1,
        routes: [],
      },
    ),
    providerRevealConfigIdsByToken: new Map(),
    providerRevealCounter: 0,
    providerSettings: cloneResponse(options.providerSettingsList ?? fixtureProviderSettingsList),
    subscriptionCounter: 0,
    worktreePagesByConversation,
  }

  state.fixtureEventState = {
    getListener: () => state.batchListener,
    getSubscription: () => state.activeSubscription,
    trackTimeout: (timeoutId, resolve) => {
      state.pendingBatchTimeouts.set(timeoutId, resolve)
    },
    untrackTimeout: (timeoutId) => {
      state.pendingBatchTimeouts.delete(timeoutId)
    },
  }
  state.clearPendingBatches = () => {
    for (const [timeoutId, resolve] of state.pendingBatchTimeouts) {
      window.clearTimeout(timeoutId)
      resolve()
    }
    state.pendingBatchTimeouts.clear()
  }
  state.emitCatalogInstallProgress = (request, stage, percent) => {
    if (!request.operationId) {
      return
    }

    const payload = {
      entryId: request.entryId,
      operationId: request.operationId,
      percent,
      sourceId: request.sourceId,
      stage,
      version: request.version,
    } satisfies SkillCatalogInstallProgressPayload
    for (const listener of state.catalogInstallProgressListeners) {
      listener(payload)
    }
  }

  return state
}
