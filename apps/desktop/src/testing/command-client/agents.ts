import type {
  BackgroundAgentRecord,
  GetExecutionSettingsResponse,
  ListBackgroundAgentsResponse,
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

const fixtureBackgroundAgent: BackgroundAgentRecord = {
  backgroundAgentId: 'bg-agent-001',
  conversationId: 'conversation-001',
  createdAt: '2026-06-30T00:00:00.000Z',
  parentRunId: 'run-001',
  pendingInputRequestId: 'request-001',
  state: 'running',
  title: 'Run checks',
  updatedAt: '2026-06-30T00:01:00.000Z',
}

export const fixtureBackgroundAgents: ListBackgroundAgentsResponse = {
  agents: [fixtureBackgroundAgent],
}

export const agentOrchestrationBackgroundAgentsResponse: ListBackgroundAgentsResponse = {
  agents: [
    {
      backgroundAgentId: 'bg-agent-runtime-e2e',
      conversationId: 'conversation-agent-orchestration',
      createdAt: '2026-06-30T00:00:00.000Z',
      parentRunId: 'run-agent-orchestration',
      state: 'running',
      title: 'Runtime orchestration background run',
      updatedAt: '2026-06-30T00:01:00.000Z',
    },
    {
      backgroundAgentId: 'bg-agent-runtime-recovery',
      conversationId: 'conversation-agent-orchestration',
      createdAt: '2026-06-30T00:02:00.000Z',
      parentRunId: 'run-agent-recovery',
      state: 'interrupted',
      title: 'Recovered background run',
      updatedAt: '2026-06-30T00:03:00.000Z',
    },
  ],
}
