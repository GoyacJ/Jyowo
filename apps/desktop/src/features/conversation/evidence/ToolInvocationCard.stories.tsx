import type { ToolAttempt } from '@/shared/tauri/commands'
import { ToolInvocationCard } from './ToolInvocationCard'

function makeAttempt(overrides?: Partial<ToolAttempt>): ToolAttempt {
  return {
    id: 'tool-1',
    order: 0,
    toolUseId: 'tu-1',
    toolName: 'shell',
    status: 'completed',
    origin: 'builtin',
    ...overrides,
  }
}

export default { component: ToolInvocationCard, title: 'Features/Evidence/ToolInvocationCard' }

export const Running = {
  render: () => (
    <ToolInvocationCard attempt={makeAttempt({ status: 'running', durationMs: 1500 })} />
  ),
}
export const Completed = {
  render: () => (
    <ToolInvocationCard
      attempt={makeAttempt({
        status: 'completed',
        durationMs: 1200,
        outputSummary: 'test result: ok',
      })}
    />
  ),
}
export const Failed = {
  render: () => (
    <ToolInvocationCard
      attempt={makeAttempt({
        status: 'failed',
        failurePhase: 'execution',
        failureSummary: 'command not found',
      })}
    />
  ),
}
export const WithPermission = {
  render: () => (
    <ToolInvocationCard
      attempt={makeAttempt({
        status: 'waitingPermission',
        permission: {
          id: 'p-1',
          requestId: 'r-1',
          status: 'pending',
          operation: 'execute',
          target: { kind: 'command', label: 'rm -rf' },
          riskLevel: 'high',
          reason: 'Destructive',
          policy: { mode: 'default' },
          decisionOptions: [],
          dataExposure: {
            sendsWorkspaceData: false,
            sendsNetworkData: false,
            touchesPrivatePath: true,
            secretRisk: 'none',
          },
        },
      })}
    />
  ),
}
