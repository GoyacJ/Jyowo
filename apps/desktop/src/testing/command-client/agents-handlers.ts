import type {
  BackgroundAgentActionResponse,
  BackgroundAgentRecord,
  CancelRunResponse,
  DeleteAgentProfileResponse,
  DeleteBackgroundAgentResponse,
  GetBackgroundAgentResponse,
  ListAgentProfilesResponse,
  SaveAgentProfileResponse,
} from '@/shared/tauri/commands'

import { fixtureExecutionSettings, fixtureSetExecutionSettings } from './agents'
import { wait } from './base'
import type { TestCommandClientState, TestCommandHandlers } from './state'

type AgentCommandKeys =
  | 'archiveBackgroundAgent'
  | 'cancelRun'
  | 'cancelBackgroundAgent'
  | 'deleteAgentProfile'
  | 'deleteBackgroundAgent'
  | 'getBackgroundAgent'
  | 'getExecutionSettings'
  | 'listAgentProfiles'
  | 'listBackgroundAgents'
  | 'pauseBackgroundAgent'
  | 'resumeBackgroundAgent'
  | 'saveAgentProfile'
  | 'sendBackgroundAgentInput'
  | 'setExecutionSettings'

function backgroundAgentActionResponse(
  agents: TestCommandClientState['backgroundAgents'],
  backgroundAgentId: string,
  state: BackgroundAgentRecord['state'],
): BackgroundAgentActionResponse {
  const agent = agents.agents.find(
    (currentAgent) => currentAgent.backgroundAgentId === backgroundAgentId,
  )

  if (!agent) {
    throw new Error(`Background agent not found: ${backgroundAgentId}`)
  }

  return {
    agent: {
      ...agent,
      state,
      updatedAt: new Date().toISOString(),
    },
  }
}

export function createAgentCommandHandlers(
  state: TestCommandClientState,
): TestCommandHandlers<AgentCommandKeys> {
  return {
    async archiveBackgroundAgent(request) {
      await wait(state.options.delayMs)
      const response = backgroundAgentActionResponse(
        state.backgroundAgents,
        request.backgroundAgentId,
        'archived',
      )
      state.backgroundAgents = {
        agents: state.backgroundAgents.agents.map((agent) =>
          agent.backgroundAgentId === response.agent.backgroundAgentId ? response.agent : agent,
        ),
      }
      return response
    },
    async cancelBackgroundAgent(request) {
      await wait(state.options.delayMs)
      const response = backgroundAgentActionResponse(
        state.backgroundAgents,
        request.backgroundAgentId,
        'cancelled',
      )
      state.backgroundAgents = {
        agents: state.backgroundAgents.agents.map((agent) =>
          agent.backgroundAgentId === response.agent.backgroundAgentId ? response.agent : agent,
        ),
      }
      return response
    },
    async cancelRun(runId) {
      await wait(state.options.delayMs)
      return { runId, status: 'cancelled' } satisfies CancelRunResponse
    },
    async deleteAgentProfile(id) {
      await wait(state.options.delayMs)
      return { id, status: 'deleted' } satisfies DeleteAgentProfileResponse
    },
    async deleteBackgroundAgent(request) {
      await wait(state.options.delayMs)
      state.backgroundAgents = {
        agents: state.backgroundAgents.agents.filter(
          (agent) => agent.backgroundAgentId !== request.backgroundAgentId,
        ),
      }
      return {
        backgroundAgentId: request.backgroundAgentId,
        status: 'deleted',
      } satisfies DeleteBackgroundAgentResponse
    },
    async getBackgroundAgent(request) {
      await wait(state.options.delayMs)
      const agent = state.backgroundAgents.agents.find(
        (currentAgent) =>
          currentAgent.backgroundAgentId === request.backgroundAgentId &&
          (request.conversationId === undefined ||
            currentAgent.conversationId === request.conversationId),
      )
      if (!agent) {
        throw new Error(`Background agent not found: ${request.backgroundAgentId}`)
      }
      return { agent } satisfies GetBackgroundAgentResponse
    },
    async getExecutionSettings(_request) {
      await wait(state.options.delayMs)
      return state.options.executionSettings ?? fixtureExecutionSettings
    },
    async listAgentProfiles() {
      await wait(state.options.delayMs)
      return { profiles: [] } satisfies ListAgentProfilesResponse
    },
    async listBackgroundAgents(request) {
      await wait(state.options.delayMs)
      const agents = request.includeArchived
        ? state.backgroundAgents.agents
        : state.backgroundAgents.agents.filter((agent) => agent.state !== 'archived')
      return {
        agents:
          request.conversationId === undefined
            ? agents
            : agents.filter((agent) => agent.conversationId === request.conversationId),
      }
    },
    async pauseBackgroundAgent(request) {
      await wait(state.options.delayMs)
      const response = backgroundAgentActionResponse(
        state.backgroundAgents,
        request.backgroundAgentId,
        'paused',
      )
      state.backgroundAgents = {
        agents: state.backgroundAgents.agents.map((agent) =>
          agent.backgroundAgentId === response.agent.backgroundAgentId ? response.agent : agent,
        ),
      }
      return response
    },
    async resumeBackgroundAgent(request) {
      await wait(state.options.delayMs)
      const response = backgroundAgentActionResponse(
        state.backgroundAgents,
        request.backgroundAgentId,
        'running',
      )
      state.backgroundAgents = {
        agents: state.backgroundAgents.agents.map((agent) =>
          agent.backgroundAgentId === response.agent.backgroundAgentId ? response.agent : agent,
        ),
      }
      return response
    },
    async saveAgentProfile(profile) {
      await wait(state.options.delayMs)
      return { profile, status: 'saved' } satisfies SaveAgentProfileResponse
    },
    async sendBackgroundAgentInput(request) {
      await wait(state.options.delayMs)
      const response = backgroundAgentActionResponse(
        state.backgroundAgents,
        request.backgroundAgentId,
        'running',
      )
      state.backgroundAgents = {
        agents: state.backgroundAgents.agents.map((agent) =>
          agent.backgroundAgentId === response.agent.backgroundAgentId ? response.agent : agent,
        ),
      }
      return response
    },
    async setExecutionSettings(request) {
      await wait(state.options.delayMs)
      return (
        state.options.setExecutionSettings ?? {
          ...fixtureSetExecutionSettings,
          agentCapabilities: {
            ...fixtureSetExecutionSettings.agentCapabilities,
            agentTeamsEnabled: request.agentTeamsEnabled,
            backgroundAgentsEnabled: request.backgroundAgentsEnabled,
            subagentsEnabled: request.subagentsEnabled,
          },
          contextCompressionTriggerRatio: request.contextCompressionTriggerRatio,
          permissionMode: request.permissionMode,
          toolProfile: request.toolProfile,
        }
      )
    },
  }
}
