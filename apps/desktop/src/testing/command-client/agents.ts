import type {
  GetExecutionSettingsResponse,
  SetExecutionSettingsResponse,
} from '@/shared/tauri/commands'

const fixtureAgentCapabilities = {
  agentTeamsAvailable: false,
  agentTeamsEnabled: false,
  backgroundAgentsAvailable: false,
  backgroundAgentsEnabled: false,
  subagentsAvailable: false,
  subagentsEnabled: false,
  unavailableReasons: [],
}

export const fixtureExecutionSettings: GetExecutionSettingsResponse = {
  agentCapabilities: fixtureAgentCapabilities,
  autoModeAvailable: false,
  contextCompressionTriggerRatio: 0.8,
  permissionMode: 'default',
  scope: 'global',
  toolProfile: 'full',
}

export const fixtureSetExecutionSettings: SetExecutionSettingsResponse = {
  agentCapabilities: fixtureAgentCapabilities,
  autoModeAvailable: false,
  contextCompressionTriggerRatio: 0.8,
  permissionMode: 'default',
  scope: 'global',
  toolProfile: 'full',
}
