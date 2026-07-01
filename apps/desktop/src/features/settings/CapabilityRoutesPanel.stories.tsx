import type { Decorator, Meta, StoryObj } from '@storybook/react-vite'
import type { ReactNode } from 'react'

import type {
  CommandClient,
  ListProviderCapabilityRouteOptionsResponse,
  ListProviderCapabilityRoutesResponse,
  ListProviderSettingsResponse,
} from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'
import { ProviderSettingsForm } from './ProviderSettingsForm'

const meta = {
  title: 'Settings/CapabilityRoutesPanel',
  component: ProviderSettingsForm,
  parameters: {
    layout: 'centered',
  },
} satisfies Meta<typeof ProviderSettingsForm>

export default meta

type Story = StoryObj<typeof meta>

const withClient =
  (client: CommandClient): Decorator =>
  (StoryComponent) => (
    <StoryFrame>
      <CommandClientProvider client={client}>
        <StoryComponent />
      </CommandClientProvider>
    </StoryFrame>
  )

const openAiProfile = {
  protocol: 'responses',
  baseUrl: 'https://api.openai.com',
  displayName: 'OpenAI',
  hasApiKey: true,
  id: 'openai',
  isDefault: true,
  modelDescriptor: {
    protocol: 'responses',
    conversationCapability: {
      inputModalities: ['text', 'image'],
      outputModalities: ['text'],
      contextWindow: 128000,
      maxOutputTokens: 16384,
      streaming: true,
      toolCalling: true,
      reasoning: false,
      promptCache: false,
      structuredOutput: true,
    },
    contextWindow: 128000,
    displayName: 'GPT-5.4 mini',
    lifecycle: { kind: 'stable' },
    maxOutputTokens: 16384,
    modelId: 'gpt-5.4-mini',
    runtimeStatus: { kind: 'runnable' },
  },
  modelId: 'gpt-5.4-mini',
  providerId: 'openai',
} satisfies ListProviderSettingsResponse['configs'][number]

const minimaxProfile = {
  protocol: 'chat_completions',
  baseUrl: 'https://api.minimax.io',
  displayName: 'Minimax',
  hasApiKey: true,
  id: 'minimax',
  isDefault: false,
  modelDescriptor: {
    protocol: 'chat_completions',
    conversationCapability: {
      inputModalities: ['text'],
      outputModalities: ['text'],
      contextWindow: 1000000,
      maxOutputTokens: 8192,
      streaming: true,
      toolCalling: true,
      reasoning: true,
      promptCache: false,
      structuredOutput: false,
    },
    contextWindow: 1000000,
    displayName: 'MiniMax M3',
    lifecycle: { kind: 'stable' },
    maxOutputTokens: 8192,
    modelId: 'MiniMax-M3',
    runtimeStatus: { kind: 'runnable' },
  },
  modelId: 'MiniMax-M3',
  providerId: 'minimax',
} satisfies ListProviderSettingsResponse['configs'][number]

const providerSettingsWithRoutes = {
  defaultConfigId: 'openai',
  configs: [openAiProfile, minimaxProfile],
} satisfies ListProviderSettingsResponse

const providerCapabilityRouteOptions = {
  options: [
    {
      kind: 'image_generation',
      configId: 'minimax',
      providerId: 'minimax',
      operationId: 'minimax.image_generation',
      outputArtifact: 'image',
      execution: 'sync',
      costRisk: 'high',
      runtimeSupported: true,
    },
    {
      kind: 'video_generation',
      configId: 'minimax',
      providerId: 'minimax',
      operationId: 'minimax.video_generation',
      outputArtifact: 'video',
      execution: 'async_job',
      costRisk: 'high',
      runtimeSupported: true,
    },
  ],
} satisfies ListProviderCapabilityRouteOptionsResponse

const savedProviderCapabilityRoutes = {
  version: 1,
  routes: [
    {
      kind: 'image_generation',
      configId: 'minimax',
      providerId: 'minimax',
      operationIds: ['minimax.image_generation'],
      enabled: true,
    },
  ],
} satisfies ListProviderCapabilityRoutesResponse

function createCapabilityRouteStoryClient(overrides: Partial<CommandClient> = {}) {
  return {
    ...createTestCommandClient({
      providerCapabilityRouteOptions,
      providerCapabilityRoutes: { version: 1, routes: [] },
      providerSettingsList: providerSettingsWithRoutes,
    }),
    ...overrides,
  } satisfies CommandClient
}

export const Ready: Story = {
  decorators: [withClient(createCapabilityRouteStoryClient())],
}

export const Saved: Story = {
  decorators: [
    withClient(
      createCapabilityRouteStoryClient({
        listProviderCapabilityRoutes: async () => savedProviderCapabilityRoutes,
      }),
    ),
  ],
}

export const Empty: Story = {
  decorators: [
    withClient(
      createTestCommandClient({
        providerCapabilityRouteOptions: { options: [] },
        providerCapabilityRoutes: { version: 1, routes: [] },
        providerSettingsList: {
          defaultConfigId: 'openai',
          configs: [openAiProfile],
        },
      }),
    ),
  ],
}

export const Loading: Story = {
  decorators: [
    withClient(
      createCapabilityRouteStoryClient({
        listProviderCapabilityRouteOptions: () => new Promise(() => undefined),
        listProviderCapabilityRoutes: () => new Promise(() => undefined),
      }),
    ),
  ],
}

export const ErrorState: Story = {
  decorators: [
    withClient(
      createCapabilityRouteStoryClient({
        listProviderCapabilityRouteOptions: async () => {
          throw new Error('Capability route settings unavailable.')
        },
      }),
    ),
  ],
}

function StoryFrame({ children }: { children: ReactNode }) {
  return <main className="w-[760px] bg-background p-6 text-foreground">{children}</main>
}
