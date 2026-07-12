import type {
  DeleteProjectResponse,
  DeleteProviderCapabilityRouteResponse,
  ListAutomationRunsResponse,
  ListAutomationsResponse,
  ModelSettingsPageResponse,
  RefreshModelProviderCatalogResponse,
  RequestProviderConfigApiKeyRevealResponse,
  RunEvalCaseResponse,
  SaveProviderCapabilityRouteResponse,
  SwitchProjectResponse,
} from '@/shared/tauri/commands'

import { cloneResponse, timestamp, wait } from './base'
import {
  fixtureAutomation,
  fixtureAutomationRun,
  fixtureGetModelUsageSummary,
  fixtureListEvalCases,
  fixtureListOfficialQuotaSnapshots,
  fixtureListProviderProbeSnapshots,
  fixtureModelProviderCatalog,
  fixtureProbeProviderConfig,
  fixtureProviderApiKeyForConfig,
  fixtureRefreshOfficialQuota,
  fixtureSaveProviderSettings,
  fixtureValidateProviderSettings,
  normalizeAutomationSpec,
} from './settings'
import type { TestCommandClientState, TestCommandHandlers } from './state'

type SettingsCommandKeys =
  | 'addProject'
  | 'deleteProject'
  | 'deleteProviderCapabilityRoute'
  | 'getProviderConfigApiKey'
  | 'getModelSettingsPage'
  | 'getModelUsageSummary'
  | 'listOfficialQuotaSnapshots'
  | 'listModelProviderCatalog'
  | 'listProjects'
  | 'getDefaultWorkspace'
  | 'listProviderCapabilityRouteOptions'
  | 'listProviderCapabilityRoutes'
  | 'listProviderProbeSnapshots'
  | 'listProviderSettings'
  | 'moveProject'
  | 'renameProject'
  | 'probeProviderConfig'
  | 'refreshModelProviderCatalog'
  | 'refreshOfficialQuota'
  | 'requestProviderConfigApiKeyReveal'
  | 'saveProviderCapabilityRoute'
  | 'saveProviderSettings'
  | 'switchProject'
  | 'validateProviderSettings'

export function createSettingsCommandHandlers(
  state: TestCommandClientState,
): TestCommandHandlers<SettingsCommandKeys> {
  return {
    async addProject(path) {
      await wait(state.options.delayMs)
      const name = path.split(/[\\/]/).filter(Boolean).at(-1) ?? 'Project'
      const project = {
        lastOpenedAt: new Date().toISOString(),
        name,
        path,
      } satisfies SwitchProjectResponse['project']
      state.projects = {
        activePath: path,
        projects: [project, ...state.projects.projects.filter((entry) => entry.path !== path)],
      }
      return { project }
    },
    async deleteProject(path) {
      await wait(state.options.delayMs)
      const removed = state.projects.projects.find((entry) => entry.path === path)
      if (!removed) {
        throw new Error(`Project not found: ${path}`)
      }
      const activePath = state.projects.activePath === path ? null : state.projects.activePath
      state.projects = {
        activePath,
        projects: state.projects.projects.filter((entry) => entry.path !== path),
      }
      return {
        activePath,
        path,
        status: 'deleted',
      } satisfies DeleteProjectResponse
    },
    async getDefaultWorkspace() {
      await wait(state.options.delayMs)
      return { path: '/Users/test/.jyowo/workspaces/default' }
    },
    async deleteProviderCapabilityRoute(request) {
      await wait(state.options.delayMs)
      state.providerCapabilityRoutes = {
        version: state.providerCapabilityRoutes.version,
        routes: state.providerCapabilityRoutes.routes.filter(
          (route) =>
            !(
              route.kind === request.kind &&
              route.configId === request.configId &&
              route.providerId === request.providerId
            ),
        ),
      }
      return {
        version: state.providerCapabilityRoutes.version,
        routes: cloneResponse(state.providerCapabilityRoutes.routes),
        status: 'deleted',
      } satisfies DeleteProviderCapabilityRouteResponse
    },
    async getProviderConfigApiKey(configId, revealToken) {
      await wait(state.options.delayMs)
      const tokenConfigId = state.providerRevealConfigIdsByToken.get(revealToken)
      state.providerRevealConfigIdsByToken.delete(revealToken)
      if (tokenConfigId !== configId) {
        throw new Error('provider API key reveal token is invalid or expired')
      }
      if (state.options.providerConfigApiKey) {
        return {
          ...state.options.providerConfigApiKey,
          configId,
        }
      }
      return {
        apiKey: fixtureProviderApiKeyForConfig(configId),
        configId,
      }
    },
    async getModelUsageSummary() {
      await wait(state.options.delayMs)
      return cloneResponse(state.options.modelUsageSummary ?? fixtureGetModelUsageSummary)
    },
    async getModelSettingsPage() {
      await wait(state.options.delayMs)
      return cloneResponse(
        state.options.modelSettingsPage ??
          ({
            catalog: state.options.modelProviderCatalog ?? fixtureModelProviderCatalog,
            catalogSnapshot: { source: 'bundled' },
            providerSettings: state.providerSettings,
            probeSnapshots: {
              status: 'ready',
              data: state.options.providerProbeSnapshots ?? fixtureListProviderProbeSnapshots,
            },
            usageSummary: {
              status: 'ready',
              data: state.options.modelUsageSummary ?? fixtureGetModelUsageSummary,
            },
            quotaSnapshots: {
              status: 'ready',
              data: state.options.officialQuotaSnapshots ?? fixtureListOfficialQuotaSnapshots,
            },
            capabilityRoutes: { status: 'ready', data: state.providerCapabilityRoutes },
            capabilityRouteOptions: {
              status: 'ready',
              data: state.providerCapabilityRouteOptions,
            },
            generatedAt: timestamp,
          } satisfies ModelSettingsPageResponse),
      )
    },
    async listModelProviderCatalog() {
      await wait(state.options.delayMs)
      return state.options.modelProviderCatalog ?? fixtureModelProviderCatalog
    },
    async listOfficialQuotaSnapshots() {
      await wait(state.options.delayMs)
      return cloneResponse(
        state.options.officialQuotaSnapshots ?? fixtureListOfficialQuotaSnapshots,
      )
    },
    async listProjects() {
      await wait(state.options.delayMs)
      return state.projects
    },
    async listProviderCapabilityRouteOptions() {
      await wait(state.options.delayMs)
      return cloneResponse(state.providerCapabilityRouteOptions)
    },
    async listProviderCapabilityRoutes() {
      await wait(state.options.delayMs)
      return cloneResponse(state.providerCapabilityRoutes)
    },
    async listProviderProbeSnapshots() {
      await wait(state.options.delayMs)
      return cloneResponse(
        state.options.providerProbeSnapshots ?? fixtureListProviderProbeSnapshots,
      )
    },
    async listProviderSettings(_workspaceRoot?: string) {
      await wait(state.options.delayMs)
      return cloneResponse(state.providerSettings)
    },
    async probeProviderConfig() {
      await wait(state.options.delayMs)
      return cloneResponse(state.options.providerProbe ?? fixtureProbeProviderConfig)
    },
    async refreshOfficialQuota() {
      await wait(state.options.delayMs)
      return cloneResponse(state.options.officialQuotaRefresh ?? fixtureRefreshOfficialQuota)
    },
    async refreshModelProviderCatalog() {
      await wait(state.options.delayMs)
      return cloneResponse(
        state.options.modelProviderCatalogRefresh ??
          ({
            catalog: state.options.modelProviderCatalog ?? fixtureModelProviderCatalog,
            catalogSnapshot: { source: 'bundled' },
          } satisfies RefreshModelProviderCatalogResponse),
      )
    },
    async requestProviderConfigApiKeyReveal(configId) {
      await wait(state.options.delayMs)
      const config = state.providerSettings.configs.find(
        (currentConfig) => currentConfig.id === configId,
      )
      if (!config?.hasApiKey) {
        throw new Error(`provider config API key is not configured: ${configId}`)
      }
      state.providerRevealCounter += 1
      const response: RequestProviderConfigApiKeyRevealResponse = state.options
        .providerConfigApiKeyReveal ?? {
        configId,
        expiresInSeconds: 60,
        revealToken: `fixture-reveal-token-${state.providerRevealCounter}`,
        status: 'ready',
      }
      state.providerRevealConfigIdsByToken.set(response.revealToken, configId)
      return {
        ...response,
        configId,
      }
    },
    async saveProviderCapabilityRoute(request) {
      await wait(state.options.delayMs)
      const nextRoutes = state.providerCapabilityRoutes.routes.filter(
        (route) =>
          !(
            route.kind === request.route.kind &&
            route.configId === request.route.configId &&
            route.providerId === request.route.providerId
          ),
      )
      if (request.route.enabled) {
        nextRoutes.push(request.route)
      }
      state.providerCapabilityRoutes = {
        version: state.providerCapabilityRoutes.version,
        routes: nextRoutes.sort((left, right) =>
          `${left.kind}:${left.configId}`.localeCompare(`${right.kind}:${right.configId}`),
        ),
      }
      return {
        version: state.providerCapabilityRoutes.version,
        routes: cloneResponse(state.providerCapabilityRoutes.routes),
        status: 'saved',
      } satisfies SaveProviderCapabilityRouteResponse
    },
    async saveProviderSettings() {
      await wait(state.options.delayMs)
      const response = state.options.providerSettings ?? fixtureSaveProviderSettings
      state.providerSettings = {
        defaultConfigId: response.config.isDefault
          ? response.config.id
          : state.providerSettings.defaultConfigId,
        selectionScope: state.providerSettings.selectionScope,
        configs: [
          ...state.providerSettings.configs.filter((config) => config.id !== response.config.id),
          response.config,
        ]
          .map((config) =>
            response.config.isDefault
              ? {
                  ...config,
                  isDefault: config.id === response.config.id,
                }
              : config,
          )
          .sort((left, right) => left.id.localeCompare(right.id)),
      }
      return response
    },
    async switchProject(path) {
      await wait(state.options.delayMs)
      const project = state.projects.projects.find((entry) => entry.path === path)
      if (!project) {
        throw new Error(`Project not found: ${path}`)
      }
      state.projects = {
        ...state.projects,
        activePath: path,
      }
      return { project }
    },
    async moveProject(path, direction) {
      await wait(state.options.delayMs)
      const index = state.projects.projects.findIndex((entry) => entry.path === path)
      if (index < 0) {
        throw new Error(`Project not found: ${path}`)
      }
      const targetIndex =
        direction === 'up'
          ? Math.max(0, index - 1)
          : Math.min(state.projects.projects.length - 1, index + 1)
      const projects = [...state.projects.projects]
      if (targetIndex !== index) {
        const [project] = projects.splice(index, 1)
        projects.splice(targetIndex, 0, project)
      }
      state.projects = {
        ...state.projects,
        projects,
      }
      return state.projects
    },
    async renameProject(path, name) {
      await wait(state.options.delayMs)
      const project = state.projects.projects.find((entry) => entry.path === path)
      if (!project) throw new Error(`Project not found: ${path}`)
      project.name = name.trim()
      return { project }
    },
    async validateProviderSettings() {
      await wait(state.options.delayMs)
      return state.options.providerValidation ?? fixtureValidateProviderSettings
    },
  }
}
