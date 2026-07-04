import { DecisionPanel } from './DecisionPanel'
import type { DecisionRequestState } from '@/shared/tauri/commands'

function makeDecision(overrides?: Partial<DecisionRequestState>): DecisionRequestState {
  return {
    id: 'perm-1',
    requestId: 'req-1',
    status: 'pending',
    operation: 'execute',
    target: { kind: 'command', label: 'cargo test' },
    riskLevel: 'medium',
    reason: 'Shell command requires approval',
    policy: { mode: 'default' },
    decisionOptions: [
      { id: 'opt-1', decision: 'approve', label: 'Allow once', lifetime: 'once', matcher: { kind: 'exactCommand', label: 'cargo test' }, requiresConfirmation: false },
      { id: 'opt-2', decision: 'approve', label: 'Allow all shell', lifetime: 'session', matcher: { kind: 'toolName', label: 'shell' }, requiresConfirmation: false },
    ],
    dataExposure: { sendsWorkspaceData: false, sendsNetworkData: false, touchesPrivatePath: false, secretRisk: 'none' },
    ...overrides,
  }
}

export default { component: DecisionPanel, title: 'Features/Evidence/DecisionPanel' }

export const LowRisk = { render: () => <DecisionPanel conversationId="conv-1" decision={makeDecision({ riskLevel: 'low' })} /> }
export const HighRisk = { render: () => <DecisionPanel conversationId="conv-1" decision={makeDecision({ riskLevel: 'high', confirmation: { expectedText: 'I understand', label: 'Type to confirm' } })} /> }
export const Critical = { render: () => <DecisionPanel conversationId="conv-1" decision={makeDecision({ riskLevel: 'critical', reason: 'This action deletes files permanently' })} /> }
export const Submitting = { render: () => <DecisionPanel conversationId="conv-1" decision={makeDecision({ status: 'submitting' })} /> }
export const Failed = { render: () => <DecisionPanel conversationId="conv-1" decision={makeDecision({ status: 'failed', reason: 'Network timeout during submission' })} /> }
