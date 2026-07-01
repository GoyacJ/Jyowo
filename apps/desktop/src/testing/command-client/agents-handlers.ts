import type { CancelRunResponse, SetConversationModelConfigResponse } from '@/shared/tauri/commands'

import { fixtureExecutionSettings, fixtureSetExecutionSettings } from './agents'
import { wait } from './base'
import type { TestCommandClientState, TestCommandHandlers } from './state'

type AgentCommandKeys =
  | 'cancelRun'
  | 'getExecutionSettings'
  | 'setConversationModelConfig'
  | 'setExecutionSettings'

export function createAgentCommandHandlers(
  state: TestCommandClientState,
): TestCommandHandlers<AgentCommandKeys> {
  return {
    async cancelRun(runId) {
      await wait(state.options.delayMs)
      return { runId, status: 'cancelled' } satisfies CancelRunResponse
    },
    async getExecutionSettings(_request) {
      await wait(state.options.delayMs)
      return state.options.executionSettings ?? fixtureExecutionSettings
    },
    async setConversationModelConfig(conversationId, modelConfigId) {
      await wait(state.options.delayMs)
      return {
        conversationId,
        modelConfigId,
        status: 'saved',
      } satisfies SetConversationModelConfigResponse
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
