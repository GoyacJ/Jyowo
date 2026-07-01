import type {
  DeleteProjectResponse,
  DeleteProviderCapabilityRouteResponse,
  ListAutomationRunsResponse,
  ListAutomationsResponse,
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
  | 'deleteAutomation'
  | 'deleteProject'
  | 'deleteProviderCapabilityRoute'
  | 'getProviderConfigApiKey'
  | 'getModelUsageSummary'
  | 'listAutomationRuns'
  | 'listAutomations'
  | 'listEvalCases'
  | 'listOfficialQuotaSnapshots'
  | 'listModelProviderCatalog'
  | 'listProjects'
  | 'listProviderCapabilityRouteOptions'
  | 'listProviderCapabilityRoutes'
  | 'listProviderProbeSnapshots'
  | 'listProviderSettings'
  | 'probeProviderConfig'
  | 'refreshOfficialQuota'
  | 'requestProviderConfigApiKeyReveal'
  | 'runAutomationNow'
  | 'runEvalCase'
  | 'saveAutomation'
  | 'saveProviderCapabilityRoute'
  | 'saveProviderSettings'
  | 'setAutomationEnabled'
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
    async deleteAutomation(id) {
      await wait(state.options.delayMs)
      state.automations = {
        automations: state.automations.automations.filter((automation) => automation.id !== id),
      }
      return (
        state.options.automationDelete ?? {
          id,
          status: 'deleted',
        }
      )
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
    async listAutomationRuns(automationId) {
      await wait(state.options.delayMs)
      const runs =
        automationId === undefined
          ? state.automationRuns.runs
          : state.automationRuns.runs.filter((run) => run.automationId === automationId)
      return {
        runs: cloneResponse(runs),
      }
    },
    async listAutomations() {
      await wait(state.options.delayMs)
      return cloneResponse(state.automations)
    },
    async listEvalCases() {
      await wait(state.options.delayMs)
      return state.options.evalCases ?? fixtureListEvalCases
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
    async listProviderSettings() {
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
    async runAutomationNow(id) {
      await wait(state.options.delayMs)
      const record = {
        ...fixtureAutomationRun,
        automationId: id,
      } satisfies ListAutomationRunsResponse['runs'][number]
      state.automationRuns = {
        runs: [record, ...state.automationRuns.runs.filter((run) => run.id !== record.id)],
      }
      return (
        state.options.automationRunNow ?? {
          record,
        }
      )
    },
    async runEvalCase(caseId) {
      await wait(state.options.delayMs)
      const evalCase =
        (state.options.evalCases ?? fixtureListEvalCases).cases.find(
          (currentCase) => currentCase.id === caseId,
        ) ?? fixtureListEvalCases.cases[0]

      return {
        case: {
          ...evalCase,
          lastRun: {
            completedAt: timestamp,
            failed: 0,
            passed: (evalCase.lastRun?.passed ?? 0) + 1,
            status: 'passed',
          },
        },
        status: 'completed',
      } satisfies RunEvalCaseResponse
    },
    async saveAutomation(request) {
      await wait(state.options.delayMs)
      const automation = normalizeAutomationSpec(request.automation)
      state.automations = {
        automations: [
          automation,
          ...state.automations.automations.filter(
            (automation) => automation.id !== request.automation.id,
          ),
        ],
      }
      return (
        state.options.automationSave ?? {
          automation,
          status: 'saved',
        }
      )
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
    async setAutomationEnabled(id, enabled) {
      await wait(state.options.delayMs)
      const automation =
        state.automations.automations.find((automation) => automation.id === id) ??
        fixtureAutomation
      const updated = {
        ...automation,
        enabled,
        updatedAt: new Date().toISOString(),
      } satisfies ListAutomationsResponse['automations'][number]
      state.automations = {
        automations: [
          updated,
          ...state.automations.automations.filter((automation) => automation.id !== id),
        ],
      }
      return (
        state.options.automationSetEnabled ?? {
          automation: updated,
          status: 'saved',
        }
      )
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
    async validateProviderSettings() {
      await wait(state.options.delayMs)
      return state.options.providerValidation ?? fixtureValidateProviderSettings
    },
  }
}
