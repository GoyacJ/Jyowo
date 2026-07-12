import type {
  DeleteAgentProfileResponse,
  ListAgentProfilesResponse,
  SaveAgentProfileResponse,
} from '@/shared/tauri/commands'

import { fixtureExecutionSettings, fixtureSetExecutionSettings } from './agents'
import { wait } from './base'
import type { TestCommandClientState, TestCommandHandlers } from './state'

type AgentCommandKeys =
  | 'deleteAgentProfile'
  | 'getExecutionSettings'
  | 'listAgentProfiles'
  | 'saveAgentProfile'
  | 'setExecutionSettings'

export function createAgentCommandHandlers(
  state: TestCommandClientState,
): TestCommandHandlers<AgentCommandKeys> {
  return {
    async deleteAgentProfile(id) {
      await wait(state.options.delayMs)
      return { id, status: 'deleted' } satisfies DeleteAgentProfileResponse
    },
    async getExecutionSettings() {
      await wait(state.options.delayMs)
      return state.options.executionSettings ?? fixtureExecutionSettings
    },
    async listAgentProfiles() {
      await wait(state.options.delayMs)
      return { profiles: [] } satisfies ListAgentProfilesResponse
    },
    async saveAgentProfile(profile) {
      await wait(state.options.delayMs)
      return { profile, status: 'saved' } satisfies SaveAgentProfileResponse
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
