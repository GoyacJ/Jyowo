import type { Decorator, Meta, StoryObj } from '@storybook/react-vite'
import type { ReactNode } from 'react'

import type {
  AttachmentReference,
  ConversationModelCapability,
  ListReferenceCandidatesResponse,
} from '@/shared/tauri/commands'

import { Composer, type ComposerSubmitPayload } from './Composer'

const textImageCapability = {
  inputModalities: ['text', 'image', 'file'],
  outputModalities: ['text'],
  contextWindow: 128000,
  maxOutputTokens: 16384,
  streaming: true,
  toolCalling: true,
  reasoning: false,
  promptCache: true,
  structuredOutput: true,
} satisfies ConversationModelCapability

const textOnlyCapability = {
  ...textImageCapability,
  inputModalities: ['text'],
} satisfies ConversationModelCapability

const referenceCandidates = {
  artifacts: [{ id: 'artifact-test-architecture', label: 'Test architecture audit' }],
  conversations: [{ id: 'conversation-cleanup', label: 'Cleanup implementation thread' }],
  files: [
    {
      label: 'docs/testing/test-inventory.md',
      path: 'docs/testing/test-inventory.md',
    },
  ],
  memories: [{ id: 'memory-policy-boundary', label: 'Policy authority remains in Rust' }],
  mcpServers: [{ id: 'mcp-filesystem', label: 'Filesystem MCP' }],
  skills: [{ id: 'skill-code-review', label: 'Code review skill' }],
  tools: [{ id: 'tool-ripgrep', label: 'ripgrep' }],
} satisfies ListReferenceCandidatesResponse

const markdownAttachment = {
  blobRef: {
    contentHash: Array.from({ length: 32 }, (_, index) => index),
    contentType: 'text/markdown',
    id: 'blob-test-inventory',
    size: 18432,
  },
  id: `attachment-${'a'.repeat(64)}`,
  mimeType: 'text/markdown',
  name: 'test-inventory.md',
  sizeBytes: 18432,
} satisfies AttachmentReference

const noopSubmit = async (_draft: ComposerSubmitPayload) => {}
const noopAction = async () => {}

const withFrame: Decorator = (StoryComponent) => (
  <StoryFrame>
    <StoryComponent />
  </StoryFrame>
)

const meta = {
  title: 'Conversation/Composer',
  component: Composer,
  parameters: {
    layout: 'centered',
  },
  decorators: [withFrame],
  args: {
    autoModeAvailable: true,
    modelCapability: textImageCapability,
    modelConfigId: 'openai-work',
    modelConfigs: [
      { id: 'openai-work', label: 'OpenAI Work / GPT-5.4 mini' },
      { id: 'local-review', label: 'Local Review / Llama 3.1' },
    ],
    onCancelRun: noopAction,
    onCreateAttachmentFromPath: async () => ({ attachment: markdownAttachment }),
    onListReferenceCandidates: async () => referenceCandidates,
    onPickAttachmentPath: async () => '/Users/goya/Repo/Git/Jyowo/docs/testing/test-inventory.md',
    onSubmit: noopSubmit,
  },
} satisfies Meta<typeof Composer>

export default meta

type Story = StoryObj<typeof meta>

export const Ready: Story = {}

export const Loading: Story = {
  args: {
    mode: { kind: 'submitting' },
  },
}

export const Empty: Story = {
  args: {
    modelConfigDisabled: true,
    modelConfigId: '',
    modelConfigs: [],
  },
}

export const ErrorState: Story = {
  name: 'Error',
  args: {
    errorMessage: 'Runtime rejected the request before starting the run.',
    mode: { kind: 'retry' },
    onRetry: () => undefined,
  },
}

export const Submitting: Story = Loading

export const Running: Story = {
  args: {
    cancelPending: false,
    mode: { kind: 'running-disabled', canCancel: true },
  },
}

export const HighRiskPermission: Story = {
  args: {
    permissionMode: 'bypass_permissions',
  },
}

export const TextOnlyModel: Story = {
  args: {
    modelCapability: textOnlyCapability,
  },
}

function StoryFrame({ children }: { children: ReactNode }) {
  return <main className="w-[720px] bg-background p-6 text-foreground">{children}</main>
}
