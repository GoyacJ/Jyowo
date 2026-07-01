import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type {
  AgentCapabilities,
  AgentProfile,
  ConversationModelCapability,
} from '@/shared/tauri/commands'

import { Composer } from './Composer'

const useAgentProfilesMock = vi.hoisted(() => vi.fn())

vi.mock('./use-agent-profiles', () => ({
  useAgentProfiles: useAgentProfilesMock,
}))

const attachment = {
  blobRef: {
    contentHash: Array.from({ length: 32 }, () => 1),
    contentType: 'text/plain',
    id: '01J00000000000000000000000',
    size: 128,
  },
  id: 'attachment-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
  mimeType: 'text/plain',
  name: 'notes.txt',
  sizeBytes: 128,
}

const textOnlyCapability: ConversationModelCapability = {
  inputModalities: ['text'],
  outputModalities: ['text'],
  contextWindow: 128000,
  maxOutputTokens: 8192,
  streaming: true,
  toolCalling: true,
  reasoning: false,
  promptCache: false,
  structuredOutput: true,
}

const referenceCandidates = {
  artifacts: [{ id: 'artifact-001', label: 'Build notes' }],
  conversations: [],
  files: [
    {
      label: 'Composer.tsx',
      path: 'apps/desktop/src/features/conversation/Composer.tsx',
    },
  ],
  memories: [],
  mcpServers: [{ id: 'mcp-filesystem', label: 'Filesystem MCP' }],
  skills: [{ id: 'skill-review', label: 'Code review skill' }],
  tools: [{ id: 'builtin.grep', label: 'Search files' }],
}

const availableAgentCapabilities: AgentCapabilities = {
  agentTeamsAvailable: true,
  agentTeamsEnabled: true,
  backgroundAgentsAvailable: false,
  backgroundAgentsEnabled: false,
  subagentsAvailable: true,
  subagentsEnabled: true,
  unavailableReasons: [],
}

const leadProfile: AgentProfile = {
  contextMode: 'focused',
  defaultWorkspaceIsolation: 'read_only',
  description: 'Leads team runs',
  id: 'lead',
  maxDepth: 2,
  maxTurns: 8,
  memoryScope: 'read_only',
  role: 'Lead',
  sandboxInheritance: 'inherit_parent',
  scope: 'builtin',
  toolBlocklist: [],
}

const workerProfile: AgentProfile = {
  contextMode: 'minimal',
  defaultWorkspaceIsolation: 'read_only',
  description: 'Executes delegated work',
  id: 'worker',
  maxDepth: 1,
  maxTurns: 6,
  memoryScope: 'none',
  role: 'Worker',
  sandboxInheritance: 'narrow_only',
  scope: 'builtin',
  toolBlocklist: [],
}

describe('Composer', () => {
  beforeEach(() => {
    useAgentProfilesMock.mockReturnValue({
      error: null,
      isEmpty: false,
      isLoading: false,
      profiles: [leadProfile, workerProfile],
      workspacePath: '/tmp/jyowo-project',
    })
  })

  it('submits typed text as structured draft', async () => {
    const onSubmit = vi.fn()

    render(<Composer modelConfigId="provider-config-001" onSubmit={onSubmit} />)

    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project...'), {
      target: { value: 'Continue the setup' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    await waitFor(() =>
      expect(onSubmit).toHaveBeenCalledWith({
        agentOptions: undefined,
        attachments: [],
        contextReferences: [],
        modelConfigId: 'provider-config-001',
        permissionMode: 'default',
        prompt: 'Continue the setup',
      }),
    )
  })

  it('submits with Enter and keeps Shift Enter as newline', async () => {
    const onSubmit = vi.fn()

    render(<Composer modelConfigId="provider-config-001" onSubmit={onSubmit} />)

    const input = screen.getByPlaceholderText('Ask Jyowo anything about this project...')
    fireEvent.change(input, {
      target: { value: 'First line' },
    })
    fireEvent.keyDown(input, { key: 'Enter', shiftKey: true })
    fireEvent.change(input, {
      target: { value: 'First line\nSecond line' },
    })
    fireEvent.keyDown(input, { key: 'Enter' })

    await waitFor(() =>
      expect(onSubmit).toHaveBeenCalledWith({
        agentOptions: undefined,
        attachments: [],
        contextReferences: [],
        modelConfigId: 'provider-config-001',
        permissionMode: 'default',
        prompt: 'First line\nSecond line',
      }),
    )
  })

  it('submits the selected permission mode from the toolbar', async () => {
    const onSubmit = vi.fn()

    render(
      <Composer
        modelConfigId="provider-config-001"
        autoModeAvailable={false}
        onSubmit={onSubmit}
      />,
    )

    fireEvent.pointerDown(screen.getByRole('button', { name: 'Permission mode: Request approval' }))
    fireEvent.click(await screen.findByRole('menuitem', { name: /Full access/i }))
    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project...'), {
      target: { value: 'Run without prompts' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    await waitFor(() =>
      expect(onSubmit).toHaveBeenCalledWith({
        agentOptions: undefined,
        attachments: [],
        contextReferences: [],
        modelConfigId: 'provider-config-001',
        permissionMode: 'bypass_permissions',
        prompt: 'Run without prompts',
      }),
    )
  })

  it('disables auto approval in the composer when the desktop build does not support it', async () => {
    render(
      <Composer modelConfigId="provider-config-001" autoModeAvailable={false} onSubmit={vi.fn()} />,
    )

    fireEvent.pointerDown(screen.getByRole('button', { name: 'Permission mode: Request approval' }))

    expect(await screen.findByRole('menuitem', { name: /Auto approve/i })).toHaveAttribute(
      'aria-disabled',
      'true',
    )
  })

  it('does not submit Enter while IME composition is active', () => {
    const onSubmit = vi.fn()

    render(<Composer modelConfigId="provider-config-001" onSubmit={onSubmit} />)

    const input = screen.getByPlaceholderText('Ask Jyowo anything about this project...')
    fireEvent.change(input, {
      target: { value: '输入中' },
    })
    fireEvent.keyDown(input, { isComposing: true, key: 'Enter' })

    expect(onSubmit).not.toHaveBeenCalled()
  })

  it('blocks empty submit', () => {
    const onSubmit = vi.fn()

    render(<Composer modelConfigId="provider-config-001" onSubmit={onSubmit} />)

    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    expect(onSubmit).not.toHaveBeenCalled()
  })

  it('gives all context buttons accessible names', () => {
    render(<Composer modelConfigId="provider-config-001" onSubmit={vi.fn()} />)

    expect(screen.getByRole('button', { name: 'Attach file' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Reference project object' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Command mode' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '>_' })).not.toBeInTheDocument()
  })

  it('renders model selection inside the composer toolbar', () => {
    const onModelConfigChange = vi.fn()

    render(
      <Composer
        modelConfigId=""
        modelConfigs={[
          {
            id: 'openai-work',
            label: 'OpenAI Work / gpt-5.4-mini',
          },
        ]}
        onModelConfigChange={onModelConfigChange}
        onSubmit={vi.fn()}
      />,
    )

    const modelSelector = screen.getByLabelText('Model') as HTMLSelectElement
    expect(modelSelector.closest('form')).not.toBeNull()

    fireEvent.change(modelSelector, { target: { value: 'openai-work' } })

    expect(onModelConfigChange).toHaveBeenCalledWith('openai-work')
  })

  it('disables attachments when the selected model only accepts text', () => {
    render(
      <Composer
        modelConfigId="provider-config-001"
        modelCapability={textOnlyCapability}
        onSubmit={vi.fn()}
      />,
    )

    expect(screen.getByRole('button', { name: 'Attach file' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Reference project object' })).not.toBeDisabled()
  })

  it('disables attachments when the selected model capability is unknown', () => {
    render(
      <Composer modelConfigId="provider-config-001" modelCapability={null} onSubmit={vi.fn()} />,
    )

    expect(screen.getByRole('button', { name: 'Attach file' })).toBeDisabled()
  })

  it('enables attachments when the selected model accepts media or files', () => {
    render(
      <Composer
        modelConfigId="provider-config-001"
        modelCapability={{
          ...textOnlyCapability,
          inputModalities: ['text', 'image'],
        }}
        onSubmit={vi.fn()}
      />,
    )

    expect(screen.getByRole('button', { name: 'Attach file' })).not.toBeDisabled()
  })

  it('passes accepted attachment modalities to the picker', async () => {
    const onPickAttachmentPath = vi.fn().mockResolvedValue(null)

    render(
      <Composer
        modelConfigId="provider-config-001"
        modelCapability={{
          ...textOnlyCapability,
          inputModalities: ['text', 'image', 'video', 'file'],
        }}
        onCreateAttachmentFromPath={vi.fn()}
        onPickAttachmentPath={onPickAttachmentPath}
        onSubmit={vi.fn()}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Attach file' }))

    await waitFor(() =>
      expect(onPickAttachmentPath).toHaveBeenCalledWith(['image', 'video', 'file']),
    )
  })

  it('adds an attachment chip from the picker and submits it', async () => {
    const onSubmit = vi.fn()

    render(
      <Composer
        modelConfigId="provider-config-001"
        onCreateAttachmentFromPath={vi.fn().mockResolvedValue({ attachment })}
        onPickAttachmentPath={vi.fn().mockResolvedValue('/tmp/notes.txt')}
        onSubmit={onSubmit}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Attach file' }))
    expect(await screen.findByText('notes.txt')).toBeInTheDocument()

    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project...'), {
      target: { value: 'Use this file' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    await waitFor(() =>
      expect(onSubmit).toHaveBeenCalledWith(
        expect.objectContaining({
          attachments: [attachment],
        }),
      ),
    )
  })

  it('adds and removes a reference chip before submit', async () => {
    const onSubmit = vi.fn()

    render(
      <Composer
        modelConfigId="provider-config-001"
        onListReferenceCandidates={vi.fn().mockResolvedValue(referenceCandidates)}
        onSubmit={onSubmit}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Reference project object' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Composer.tsx' }))
    expect(screen.getByText('Composer.tsx')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Remove reference Composer.tsx' }))
    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project...'), {
      target: { value: 'No reference now' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    await waitFor(() =>
      expect(onSubmit).toHaveBeenCalledWith(
        expect.objectContaining({
          contextReferences: [],
        }),
      ),
    )
  })

  it('adds skill, tool, and MCP references from the picker', async () => {
    const onSubmit = vi.fn()

    render(
      <Composer
        modelConfigId="provider-config-001"
        onListReferenceCandidates={vi.fn().mockResolvedValue(referenceCandidates)}
        onSubmit={onSubmit}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Reference project object' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Code review skill' }))
    fireEvent.click(screen.getByRole('button', { name: 'Reference project object' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Search files' }))
    fireEvent.click(screen.getByRole('button', { name: 'Reference project object' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Filesystem MCP' }))

    expect(screen.getByText('Code review skill')).toBeInTheDocument()
    expect(screen.getByText('Search files')).toBeInTheDocument()
    expect(screen.getByText('Filesystem MCP')).toBeInTheDocument()

    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project...'), {
      target: { value: 'Use these capabilities' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    await waitFor(() =>
      expect(onSubmit).toHaveBeenCalledWith(
        expect.objectContaining({
          contextReferences: [
            { id: 'skill-review', kind: 'skill', label: 'Code review skill' },
            { id: 'builtin.grep', kind: 'tool', label: 'Search files' },
            {
              id: 'mcp-filesystem',
              kind: 'mcp_server',
              label: 'Filesystem MCP',
            },
          ],
        }),
      ),
    )
  })

  it('disables context controls and chip removal while pending', async () => {
    const { rerender } = render(
      <Composer
        modelConfigId="provider-config-001"
        onCreateAttachmentFromPath={vi.fn().mockResolvedValue({ attachment })}
        onPickAttachmentPath={vi.fn().mockResolvedValue('/tmp/notes.txt')}
        onSubmit={vi.fn()}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Attach file' }))
    expect(await screen.findByText('notes.txt')).toBeInTheDocument()

    rerender(<Composer modelConfigId="provider-config-001" onSubmit={vi.fn()} pending />)

    expect(screen.getByRole('button', { name: 'Attach file' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Reference project object' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Remove attachment notes.txt' })).toBeDisabled()
  })

  it('uses explicit composer modes for disabled and ready states', () => {
    const { rerender } = render(
      <Composer
        modelConfigId="provider-config-001"
        mode={{ kind: 'running-disabled' }}
        onSubmit={vi.fn()}
      />,
    )

    expect(screen.getByPlaceholderText('Ask Jyowo anything about this project...')).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Send message' })).toBeDisabled()

    rerender(
      <Composer
        modelConfigId="provider-config-001"
        mode={{ kind: 'clarification-reply' }}
        onSubmit={vi.fn()}
      />,
    )

    expect(screen.getByPlaceholderText('Ask Jyowo anything about this project...')).toBeEnabled()
  })

  it('shows a cancel action while a run is active', () => {
    const onCancelRun = vi.fn()

    render(
      <Composer
        modelConfigId="provider-config-001"
        mode={{ kind: 'running-disabled', canCancel: true }}
        onCancelRun={onCancelRun}
        onSubmit={vi.fn()}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Cancel run' }))

    expect(onCancelRun).toHaveBeenCalledTimes(1)
    expect(screen.getByRole('button', { name: 'Send message' })).toBeDisabled()
  })

  it('keeps text and chips when submit fails', async () => {
    render(
      <Composer
        modelConfigId="provider-config-001"
        onCreateAttachmentFromPath={vi.fn().mockResolvedValue({ attachment })}
        onPickAttachmentPath={vi.fn().mockResolvedValue('/tmp/notes.txt')}
        onSubmit={vi.fn().mockRejectedValue(new Error('Run failed'))}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Attach file' }))
    expect(await screen.findByText('notes.txt')).toBeInTheDocument()
    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project...'), {
      target: { value: 'Keep this draft' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    await waitFor(() => expect(screen.getByDisplayValue('Keep this draft')).toBeInTheDocument())
    expect(screen.getByText('notes.txt')).toBeInTheDocument()
  })

  it('does not render a local runtime label inside the input surface', () => {
    render(<Composer modelConfigId="provider-config-001" onSubmit={vi.fn()} />)

    expect(screen.queryByText('Local')).not.toBeInTheDocument()
  })

  it('shows retry when an error is present', () => {
    const onRetry = vi.fn()

    render(
      <Composer
        modelConfigId="provider-config-001"
        errorMessage="Run failed"
        onRetry={onRetry}
        onSubmit={vi.fn()}
      />,
    )

    expect(screen.getByText('Run failed')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }))
    expect(onRetry).toHaveBeenCalledTimes(1)
  })

  it('hides agent controls when capabilities are unavailable', () => {
    render(
      <Composer
        modelConfigId="provider-config-001"
        agentCapabilities={{
          ...availableAgentCapabilities,
          agentTeamsAvailable: false,
          backgroundAgentsAvailable: false,
          subagentsAvailable: false,
        }}
        onSubmit={vi.fn()}
      />,
    )

    expect(screen.queryByText('Subagents')).not.toBeInTheDocument()
    expect(screen.queryByText('Agent team')).not.toBeInTheDocument()
  })

  it('disables subagents when settings turn the capability off', () => {
    render(
      <Composer
        modelConfigId="provider-config-001"
        agentCapabilities={{
          ...availableAgentCapabilities,
          subagentsEnabled: false,
        }}
        onSubmit={vi.fn()}
      />,
    )

    expect(screen.getByRole('checkbox', { name: /Subagents/i })).toBeDisabled()
  })

  it('disables agent team when settings turn the capability off', () => {
    render(
      <Composer
        modelConfigId="provider-config-001"
        agentCapabilities={{
          ...availableAgentCapabilities,
          agentTeamsEnabled: false,
        }}
        onSubmit={vi.fn()}
      />,
    )

    expect(screen.getByRole('checkbox', { name: /Agent team/i })).toBeDisabled()
  })

  it('shows team profile loading, error, and empty states', () => {
    const { rerender } = render(
      <Composer
        modelConfigId="provider-config-001"
        agentCapabilities={availableAgentCapabilities}
        onSubmit={vi.fn()}
      />,
    )

    useAgentProfilesMock.mockReturnValue({
      error: null,
      isEmpty: false,
      isLoading: true,
      profiles: [],
      workspacePath: '/tmp/jyowo-project',
    })
    rerender(
      <Composer
        modelConfigId="provider-config-001"
        agentCapabilities={availableAgentCapabilities}
        onSubmit={vi.fn()}
      />,
    )
    expect(screen.getByText('Loading agent profiles...')).toBeInTheDocument()

    useAgentProfilesMock.mockReturnValue({
      error: new Error('profiles unavailable'),
      isEmpty: false,
      isLoading: false,
      profiles: [],
      workspacePath: '/tmp/jyowo-project',
    })
    rerender(
      <Composer
        modelConfigId="provider-config-001"
        agentCapabilities={availableAgentCapabilities}
        onSubmit={vi.fn()}
      />,
    )
    expect(screen.getByText('profiles unavailable')).toBeInTheDocument()

    useAgentProfilesMock.mockReturnValue({
      error: null,
      isEmpty: true,
      isLoading: false,
      profiles: [],
      workspacePath: '/tmp/jyowo-project',
    })
    rerender(
      <Composer
        modelConfigId="provider-config-001"
        agentCapabilities={availableAgentCapabilities}
        onSubmit={vi.fn()}
      />,
    )
    expect(screen.getByText('No agent profiles available.')).toBeInTheDocument()
  })

  it('submits a complete teamConfig when agent team is allowed for the run', async () => {
    const onSubmit = vi.fn()

    render(
      <Composer
        modelConfigId="provider-config-001"
        agentCapabilities={availableAgentCapabilities}
        onSubmit={onSubmit}
      />,
    )

    fireEvent.click(screen.getByRole('checkbox', { name: /Agent team/i }))
    fireEvent.click(screen.getByRole('checkbox', { name: /Worker/i }))
    fireEvent.change(screen.getByLabelText('Max turns per goal'), { target: { value: '5' } })
    fireEvent.change(screen.getByLabelText('Topology'), {
      target: { value: 'role_routed' },
    })
    fireEvent.change(screen.getByLabelText('Shared memory'), {
      target: { value: 'redacted_mailbox' },
    })
    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project...'), {
      target: { value: 'Coordinate work' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    await waitFor(() =>
      expect(onSubmit).toHaveBeenCalledWith(
        expect.objectContaining({
          agentOptions: {
            agentTeam: 'allowed',
            background: 'foreground',
            maxConcurrentSubagents: 2,
            maxDepth: 2,
            maxTeamMembers: 4,
            subagents: 'off',
            teamConfig: {
              leadProfileId: 'lead',
              maxTurnsPerGoal: 5,
              memberProfileIds: ['worker'],
              sharedMemoryPolicy: 'redacted_mailbox',
              topology: 'role_routed',
            },
            workspaceIsolation: 'read_only',
          },
          prompt: 'Coordinate work',
        }),
      ),
    )
  })

  it('clears teamConfig when agent team is toggled off', async () => {
    const onSubmit = vi.fn()

    render(
      <Composer
        modelConfigId="provider-config-001"
        agentCapabilities={availableAgentCapabilities}
        onSubmit={onSubmit}
      />,
    )

    fireEvent.click(screen.getByRole('checkbox', { name: /Agent team/i }))
    fireEvent.click(screen.getByRole('checkbox', { name: /Worker/i }))
    fireEvent.click(screen.getByRole('checkbox', { name: /Agent team/i }))
    fireEvent.click(screen.getByRole('checkbox', { name: /Subagents/i }))
    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project...'), {
      target: { value: 'Delegate without team' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    await waitFor(() =>
      expect(onSubmit).toHaveBeenCalledWith(
        expect.objectContaining({
          agentOptions: expect.objectContaining({
            agentTeam: 'off',
            teamConfig: null,
          }),
        }),
      ),
    )
  })

  it('rejects team submission when no member profile is selected', async () => {
    const onSubmit = vi.fn()

    render(
      <Composer
        modelConfigId="provider-config-001"
        agentCapabilities={availableAgentCapabilities}
        onSubmit={onSubmit}
      />,
    )

    fireEvent.click(screen.getByRole('checkbox', { name: /Agent team/i }))
    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project...'), {
      target: { value: 'Coordinate work' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    await waitFor(() => expect(onSubmit).not.toHaveBeenCalled())
    expect(screen.getByText('Select at least one member profile.')).toBeInTheDocument()
  })

  it('rejects stale team profile ids before submit', async () => {
    const onSubmit = vi.fn()
    const { rerender } = render(
      <Composer
        modelConfigId="provider-config-001"
        agentCapabilities={availableAgentCapabilities}
        onSubmit={onSubmit}
      />,
    )

    fireEvent.click(screen.getByRole('checkbox', { name: /Agent team/i }))
    fireEvent.click(screen.getByRole('checkbox', { name: /Worker/i }))
    useAgentProfilesMock.mockReturnValue({
      error: null,
      isEmpty: false,
      isLoading: false,
      profiles: [leadProfile],
      workspacePath: '/tmp/jyowo-project',
    })
    rerender(
      <Composer
        modelConfigId="provider-config-001"
        agentCapabilities={availableAgentCapabilities}
        onSubmit={onSubmit}
      />,
    )
    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project...'), {
      target: { value: 'Coordinate work' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    await waitFor(() => expect(onSubmit).not.toHaveBeenCalled())
    expect(screen.getByText('Selected agent profile is no longer available.')).toBeInTheDocument()
  })

  it('submits agentOptions when subagents are allowed for the run', async () => {
    const onSubmit = vi.fn()

    render(
      <Composer
        modelConfigId="provider-config-001"
        agentCapabilities={availableAgentCapabilities}
        onSubmit={onSubmit}
      />,
    )

    fireEvent.click(screen.getByRole('checkbox', { name: /Subagents/i }))
    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project...'), {
      target: { value: 'Delegate work' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    await waitFor(() =>
      expect(onSubmit).toHaveBeenCalledWith(
        expect.objectContaining({
          agentOptions: {
            agentTeam: 'off',
            background: 'foreground',
            maxConcurrentSubagents: 2,
            maxDepth: 2,
            maxTeamMembers: 4,
            subagents: 'allowed',
            teamConfig: null,
            workspaceIsolation: 'read_only',
          },
          prompt: 'Delegate work',
        }),
      ),
    )
  })

  it('omits agentOptions when no run capability is selected', async () => {
    const onSubmit = vi.fn()

    render(
      <Composer
        modelConfigId="provider-config-001"
        agentCapabilities={availableAgentCapabilities}
        onSubmit={onSubmit}
      />,
    )

    fireEvent.change(screen.getByPlaceholderText('Ask Jyowo anything about this project...'), {
      target: { value: 'Plain run' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    await waitFor(() =>
      expect(onSubmit).toHaveBeenCalledWith(
        expect.objectContaining({
          agentOptions: undefined,
          prompt: 'Plain run',
        }),
      ),
    )
  })
})
