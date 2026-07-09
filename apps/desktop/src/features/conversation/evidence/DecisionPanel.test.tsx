import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import type { DecisionRequestState } from '@/shared/tauri/commands'
import { DecisionPanel } from './DecisionPanel'

function makeDecision(overrides?: Partial<DecisionRequestState>): DecisionRequestState {
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
    const decision = makeDecision({
      decisionOptions: [
        {
          id: 'deny-1',
          decision: 'deny',
          label: 'Deny this command',
          lifetime: 'once',
          matcher: { kind: 'exactCommand', label: 'cargo test' },
          requiresConfirmation: false,
        },
      ],
    })
    render(<DecisionPanel conversationId="conv-1" decision={decision} />)
    const denyButton = screen.getByRole('button', { name: /deny/i })
    expect(denyButton).toBeDefined()
    expect(denyButton.hasAttribute('disabled')).toBe(false)
  })

  it('submits the backend-issued deny option id', () => {
    const onResolve = vi.fn()
    const decision = makeDecision({
      decisionOptions: [
        {
          id: 'deny-1',
          decision: 'deny',
          label: 'Deny this command',
          lifetime: 'once',
          matcher: { kind: 'exactCommand', label: 'cargo test' },
          requiresConfirmation: false,
        },
      ],
    })
    render(<DecisionPanel conversationId="conv-1" decision={decision} onResolve={onResolve} />)

    fireEvent.click(screen.getByRole('button', { name: /deny/i }))

    expect(onResolve).toHaveBeenCalledWith(
      expect.objectContaining({
        conversationId: 'conv-1',
        requestId: 'req-1',
        decision: 'deny',
        optionId: 'deny-1',
      }),
    )
  })

  it('does not expose a deny action without a backend-issued deny option id', () => {
    const decision = makeDecision()
    render(<DecisionPanel conversationId="conv-1" decision={decision} />)

    expect(screen.queryByRole('button', { name: /deny/i })).not.toBeInTheDocument()
  })

  it('calls onResolve with selected optionId on approve', () => {
    const onResolve = vi.fn()
    const decision = makeDecision()
    render(<DecisionPanel conversationId="conv-1" decision={decision} onResolve={onResolve} />)

    // Select first option
    fireEvent.click(screen.getByText(/Allow this command once/))
    // Click approve
    fireEvent.click(screen.getByRole('button', { name: 'Approve' }))

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

  it('renders permission resources, scope, and review details', () => {
    const decision = makeDecision({
      resources: ['command:cargo test', 'workspace:crates/jyowo-harness-tool'],
      scope: 'workspace_only',
      reviewDetails: ['execution channel: sandbox', 'network: none'],
    })
    render(<DecisionPanel conversationId="conv-1" decision={decision} />)

    expect(screen.getByText('workspace_only')).toBeInTheDocument()
    expect(screen.getByText('command:cargo test')).toBeInTheDocument()
    expect(screen.getByText('workspace:crates/jyowo-harness-tool')).toBeInTheDocument()
    expect(screen.getByText('execution channel: sandbox')).toBeInTheDocument()
    expect(screen.getByText('network: none')).toBeInTheDocument()
  })

  it('disables approve when confirmation text does not match', () => {
    const decision = makeDecision({
      riskLevel: 'high',
      confirmation: { expectedText: 'confirm', label: 'Confirm' },
    })
    render(<DecisionPanel conversationId="conv-1" decision={decision} />)
    // Select an option first
    const approveButton = screen.getByRole('button', { name: 'Approve' })
    expect(approveButton.hasAttribute('disabled')).toBe(true)
  })

  it('uses aria-live for state changes', () => {
    const decision = makeDecision()
    render(<DecisionPanel conversationId="conv-1" decision={decision} />)
    const liveRegion = screen.getByRole('status')
    expect(liveRegion).toBeDefined()
  })

  it('uses semantic risk token classes instead of hardcoded product colors', () => {
    const decision = makeDecision({ riskLevel: 'medium' })
    render(<DecisionPanel conversationId="conv-1" decision={decision} />)

    expect(screen.getByText('Medium')).toHaveClass('bg-warning/10', 'text-warning')
  })

  it('renders contract operation fallback labels without leaking i18n keys', () => {
    const decision = makeDecision({ operation: 'unknown' })
    render(<DecisionPanel conversationId="conv-1" decision={decision} />)

    expect(screen.getByText('Unknown')).toBeInTheDocument()
    expect(document.body.textContent).not.toContain('timeline.operation.unknown')
  })

  it('renders redacted and blocked secret exposure labels without raw keys', () => {
    render(
      <>
        <DecisionPanel
          conversationId="conv-1"
          decision={makeDecision({
            id: 'perm-redacted',
            requestId: 'req-redacted',
            dataExposure: {
              sendsWorkspaceData: false,
              sendsNetworkData: false,
              touchesPrivatePath: false,
              secretRisk: 'redacted',
            },
          })}
        />
        <DecisionPanel
          conversationId="conv-1"
          decision={makeDecision({
            id: 'perm-blocked',
            requestId: 'req-blocked',
            dataExposure: {
              sendsWorkspaceData: false,
              sendsNetworkData: false,
              touchesPrivatePath: false,
              secretRisk: 'blocked',
            },
          })}
        />
      </>,
    )

    expect(screen.getByText('secret redacted')).toBeInTheDocument()
    expect(screen.getByText('secret blocked')).toBeInTheDocument()
    expect(document.body.textContent).not.toContain('timeline.exposure.redacted')
    expect(document.body.textContent).not.toContain('timeline.exposure.blocked')
  })
})
