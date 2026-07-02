import type { Decorator, Meta, StoryObj } from '@storybook/react-vite'
import type { ReactNode } from 'react'

import { AppI18nProvider } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'

import { CapabilityRoutesPanel } from './CapabilityRoutesPanel'
import type { CapabilityRouteRow } from './model-settings-view-model'

uiStore.getState().setLocale('en-US')

const withStoryFrame: Decorator = (StoryComponent) => (
  <StoryFrame>
    <AppI18nProvider>
      <StoryComponent />
    </AppI18nProvider>
  </StoryFrame>
)

const meta = {
  title: 'Settings/CapabilityRoutesPanel',
  component: CapabilityRoutesPanel,
  parameters: {
    layout: 'centered',
  },
  decorators: [withStoryFrame],
  args: {
    onConfigure: () => undefined,
    routeSection: { status: 'ready', data: routeRows() },
  },
} satisfies Meta<typeof CapabilityRoutesPanel>

export default meta

type Story = StoryObj<typeof meta>

export const Loading: Story = {
  args: {
    routeSection: { status: 'loading' },
  },
}

export const Empty: Story = {
  args: {
    routeSection: { status: 'ready', data: [] },
  },
}

export const Ready: Story = {}

export const ErrorState: Story = {
  name: 'Error',
  args: {
    routeSection: {
      status: 'error',
      safeMessage: 'Route options are temporarily unavailable.',
    },
  },
}

export const Unavailable: Story = {
  args: {
    routeSection: { status: 'unavailable' },
  },
}

export const UnsupportedTarget: Story = {
  args: {
    routeSection: {
      status: 'ready',
      data: routeRows().map((row) =>
        row.kind === 'video_generation'
          ? {
              ...row,
              selectedTarget: {
                ...row.eligibleTargets[0],
                health: { status: 'unavailable' },
              },
              unavailableTargets: [
                {
                  configId: 'cfg-local',
                  providerId: 'local',
                  modelId: 'llama-local',
                  displayName: 'Local Llama',
                  operationId: 'video.generate',
                  reason: 'The local runtime does not expose async video jobs.',
                },
              ],
            }
          : row,
      ),
    },
  },
}

function StoryFrame({ children }: { children: ReactNode }) {
  return <main className="w-[880px] bg-background p-6 text-foreground">{children}</main>
}

function routeRows(): CapabilityRouteRow[] {
  return [
    {
      kind: 'image_generation',
      savedRoute: {
        kind: 'image_generation',
        configId: 'cfg-openai',
        providerId: 'openai',
        operationIds: ['images.generate'],
        enabled: true,
      },
      selectedTarget: {
        configId: 'cfg-openai',
        providerId: 'openai',
        modelId: 'gpt-4.1',
        displayName: 'Primary OpenAI',
        providerDisplayName: 'OpenAI',
        operationIds: ['images.generate'],
        execution: 'sync',
        costRisk: 'medium',
        health: {
          status: 'online',
          latencyMs: 118,
          timeoutMs: 10000,
          checkedAt: '2026-06-30T10:00:00Z',
        },
      },
      eligibleTargets: [
        {
          configId: 'cfg-openai',
          providerId: 'openai',
          modelId: 'gpt-4.1',
          displayName: 'Primary OpenAI',
          providerDisplayName: 'OpenAI',
          operationIds: ['images.generate'],
          execution: 'sync',
          costRisk: 'medium',
          health: {
            status: 'online',
            latencyMs: 118,
            timeoutMs: 10000,
            checkedAt: '2026-06-30T10:00:00Z',
          },
        },
      ],
      unavailableTargets: [],
    },
    routeRow('video_generation'),
    routeRow('speech_to_text'),
    routeRow('text_to_speech'),
    routeRow('music_generation'),
  ]
}

function routeRow(kind: CapabilityRouteRow['kind']): CapabilityRouteRow {
  return {
    kind,
    savedRoute: null,
    selectedTarget: null,
    eligibleTargets: [
      {
        configId: 'cfg-openai',
        providerId: 'openai',
        modelId: 'gpt-4.1',
        displayName: 'Primary OpenAI',
        providerDisplayName: 'OpenAI',
        operationIds: [`${kind}.run`],
        execution:
          kind === 'video_generation' || kind === 'music_generation' ? 'async_job' : 'sync',
        costRisk: kind === 'speech_to_text' ? 'low' : 'high',
        health: { status: 'never_checked' },
      },
    ],
    unavailableTargets: [],
  }
}
