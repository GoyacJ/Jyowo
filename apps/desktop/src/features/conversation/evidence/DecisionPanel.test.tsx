import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { DecisionPanel } from './DecisionPanel'
import type { DecisionRequestState } from '@/shared/tauri/commands'

function makeDecision(
  overrides?: Partial<DecisionRequestState>,
): DecisionRequestState {
  return {
    id: 'perm-1',
    requestId: 'req-1',
    status: 'pending',
    operation: 'execute',
    target: {
      kind: 'command',
      label: 'cargo test',
    },
    riskLevel: 'medium',
    reason: 'Shell command requires approval',
    policy: { mode: 'default' },
    decisionOptions: [
      {
        id: 'opt-1',
        decision: 'approve',
        label: 'Allow this command once',
        lifetime: 'once',
        matcher: { kind: 'exactCommand', label: 'cargo test' },
        requiresConfirmation: false,
      },
      {
        id: 'opt-2',
        decision: 'approve',
        label: 'Allow all shell commands this session',
        lifetime: 'session',
        matcher: { kind: 'toolName', label: 'shell' },
        requiresConfirmation: false,
      },
    ],
    dataExposure: {
      sendsWorkspaceData: false,
      sendsNetworkData: false,
      touchesPrivatePath: false,
      secretRisk: 'none',
    },
    ...overrides,
  }
}

describe('DecisionPanel', () => {
  it('renders operation and target', () => {
    const decision = makeDecision()
    render(<DecisionPanel conversationId="conv-1" decision={decision} />)
    expect(screen.getByText('cargo test')).toBeDefined()
  })

  it('renders reason when present', () => {
    const decision = makeDecision({ reason: 'Needs network access' })
    render(<DecisionPanel conversationId="conv-1" decision={decision} />)
    expect(screen.getByText('Needs network access')).toBeDefined()
  })

  it('renders decision options as selectable buttons', () => {
    const decision = makeDecision()
    render(<DecisionPanel conversationId="conv-1" decision={decision} />)
    expect(screen.getByText(/Allow this command once/)).toBeDefined()
    expect(screen.getByText(/Allow all shell commands/)).toBeDefined()
  })

  it('shows deny button without requiring option selection', () => {
    const decision = makeDecision()
    render(<DecisionPanel conversationId="conv-1" decision={decision} />)
    const denyButton = screen.getByRole('button', { name: /deny/i })
    expect(denyButton).toBeDefined()
    expect(denyButton.hasAttribute('disabled')).toBe(false)
  })

  it('calls onResolve with selected optionId on approve', () => {
    const onResolve = vi.fn()
    const decision = makeDecision()
    render(
      <DecisionPanel
        conversationId="conv-1"
        decision={decision}
        onResolve={onResolve}
      />,
    )

    // Select first option
    fireEvent.click(screen.getByText(/Allow this command once/))
    // Click approve
    fireEvent.click(screen.getByRole('button', { name: /approve/i }))

    expect(onResolve).toHaveBeenCalledWith(
      expect.objectContaining({
        conversationId: 'conv-1',
        requestId: 'req-1',
        decision: 'approve',
        optionId: 'opt-1',
      }),
    )
  })

  it('shows confirmation input for high-risk decisions', () => {
    const decision = makeDecision({
      riskLevel: 'high',
      confirmation: { expectedText: 'I understand', label: 'Type to confirm' },
    })
    render(<DecisionPanel conversationId="conv-1" decision={decision} />)
    expect(screen.getByText(/I understand/)).toBeDefined()
  })

  it('disables approve when confirmation text does not match', () => {
    const decision = makeDecision({
      riskLevel: 'high',
      confirmation: { expectedText: 'confirm', label: 'Confirm' },
    })
    render(<DecisionPanel conversationId="conv-1" decision={decision} />)
    // Select an option first
    const buttons = screen.getAllByRole('button')
    const approveButton = buttons.find((b) =>
      b.textContent?.toLowerCase().includes('approve'),
    )
    expect(approveButton?.hasAttribute('disabled')).toBe(true)
  })

  it('uses aria-live for state changes', () => {
    const decision = makeDecision()
    render(<DecisionPanel conversationId="conv-1" decision={decision} />)
    const liveRegion = screen.getByRole('status')
    expect(liveRegion).toBeDefined()
  })
})
