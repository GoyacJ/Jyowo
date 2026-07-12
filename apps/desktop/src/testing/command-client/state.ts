import type {
  CommandClient,
  InstallSkillFromCatalogRequest,
  ListProjectsResponse,
  ListProviderCapabilityRouteOptionsResponse,
  ListProviderCapabilityRoutesResponse,
  ListProviderSettingsResponse,
  SkillCatalogInstallProgressPayload,
} from '@/shared/tauri/commands'

import { cloneResponse, type TestCommandClientOptions } from './base'
import { fixtureProviderSettingsList, testJyowoProject } from './settings'

export type TestCommandHandlers<T extends keyof CommandClient> = Pick<CommandClient, T>

export type TestCommandClientState = {
  catalogInstallProgressListeners: Set<(progress: SkillCatalogInstallProgressPayload) => void>
  emitCatalogInstallProgress: (
    request: InstallSkillFromCatalogRequest,
    stage: SkillCatalogInstallProgressPayload['stage'],
    percent: number,
  ) => void
  options: TestCommandClientOptions
  projects: ListProjectsResponse
  providerCapabilityRouteOptions: ListProviderCapabilityRouteOptionsResponse
  providerCapabilityRoutes: ListProviderCapabilityRoutesResponse
  providerRevealConfigIdsByToken: Map<string, string>
  providerRevealCounter: number
  providerSettings: ListProviderSettingsResponse
}

export function createTestCommandClientState(
  options: TestCommandClientOptions,
): TestCommandClientState {
  const state: TestCommandClientState = {
    catalogInstallProgressListeners: new Set(),
    emitCatalogInstallProgress: () => undefined,
    options,
    projects: cloneResponse(options.projects ?? testJyowoProject),
    providerCapabilityRouteOptions: cloneResponse(
      options.providerCapabilityRouteOptions ?? { options: [] },
    ),
    providerCapabilityRoutes: cloneResponse(
      options.providerCapabilityRoutes ?? { routes: [], version: 1 },
    ),
    providerRevealConfigIdsByToken: new Map(),
    providerRevealCounter: 0,
    providerSettings: cloneResponse(options.providerSettingsList ?? fixtureProviderSettingsList),
  }

  state.emitCatalogInstallProgress = (request, stage, percent) => {
    if (!request.operationId) return
    const payload = {
      entryId: request.entryId,
      operationId: request.operationId,
      percent,
      sourceId: request.sourceId,
      stage,
      version: request.version,
    } satisfies SkillCatalogInstallProgressPayload
    for (const listener of state.catalogInstallProgressListeners) listener(payload)
  }

  return state
}
