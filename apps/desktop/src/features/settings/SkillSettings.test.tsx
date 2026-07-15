import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen } from '@testing-library/react'
import type { ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { AppI18nProvider } from '@/shared/i18n/i18n'
import type { SkillSummary } from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient, type TestCommandClientOptions } from '@/testing/command-client'

import { SkillSettingsPage } from './SkillSettings'

const routerSpy = vi.hoisted(() => ({
  navigate: vi.fn(async ({ search, to }: { search?: Record<string, string>; to: string }) => {
    const nextSearch = search ? `?${new URLSearchParams(search).toString()}` : ''
    window.history.pushState(null, '', `${to}${nextSearch}`)
  }),
}))

vi.mock('@tanstack/react-router', async () => ({
  useNavigate: () => routerSpy.navigate,
  useRouterState: ({
    select,
  }: {
    select: (state: { location: { search: Record<string, unknown> } }) => unknown
  }) =>
    select({
      location: {
        search: Object.fromEntries(new URLSearchParams(window.location.search)),
      },
    }),
}))

type SkillSettingsRenderOptions = Omit<TestCommandClientOptions, 'skills'> & {
  skillSummaries?: SkillSummary[]
}

function renderSkillSettingsPage(options: SkillSettingsRenderOptions = {}) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  })

  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <CommandClientProvider
        client={createTestCommandClient({
          ...options,
          skills: options.skillSummaries ? { skills: options.skillSummaries } : undefined,
        })}
      >
        <QueryClientProvider client={queryClient}>
          <AppI18nProvider>{children}</AppI18nProvider>
        </QueryClientProvider>
      </CommandClientProvider>
    )
  }

  return render(<SkillSettingsPage />, { wrapper: Wrapper })
}

describe('SkillSettingsPage', () => {
  beforeEach(() => {
    routerSpy.navigate.mockClear()
    window.history.replaceState(null, '', '/skills')
  })

  it('renders the plugins tab alongside skills, tools, and MCP', async () => {
    renderSkillSettingsPage({
      runtimeTools: {
        generation: 7,
        scope: 'project',
        customized: false,
        tools: [
          {
            name: 'MiniMaxTextToImage',
            displayName: 'MiniMax text to image',
            description: 'Generate images with MiniMax.',
            category: 'builtin',
            group: 'network',
            groupLabel: 'Network',
            originKind: 'builtin',
            originId: null,
            access: 'mutating',
            executionChannel: 'httpBroker',
            requiredCapabilities: ['provider_credential_resolver'],
            deferPolicy: 'alwaysLoad',
            longRunning: true,
            serviceBinding: {
              providerId: 'minimax',
              operationId: 'minimax.image_generation',
              routeKind: 'imageGeneration',
            },
            configuredEnabled: true,
            available: false,
            unavailableReason: 'HTTP broker is not registered',
          },
        ],
      },
    })

    expect(screen.getByRole('tab', { name: '技能' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: '工具' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'MCP' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: '插件' })).toBeInTheDocument()

    fireEvent.mouseDown(screen.getByRole('tab', { name: '插件' }))

    expect(await screen.findByRole('heading', { name: '插件' })).toBeInTheDocument()

    fireEvent.mouseDown(screen.getByRole('tab', { name: '工具' }))

    expect(await screen.findByRole('heading', { name: '工具' })).toBeInTheDocument()
    expect(await screen.findByText('MiniMaxTextToImage')).toBeInTheDocument()
  })

  it('localizes known runtime tool group labels', async () => {
    renderSkillSettingsPage({
      runtimeTools: {
        generation: 8,
        scope: 'project',
        customized: false,
        tools: [
          {
            name: 'FileRead',
            displayName: 'File read',
            description: 'Read a workspace file.',
            category: 'builtin',
            group: 'fileSystem',
            groupLabel: 'File system',
            originKind: 'builtin',
            originId: null,
            access: 'readOnly',
            executionChannel: 'directAuthorizedRust',
            requiredCapabilities: [],
            deferPolicy: 'alwaysLoad',
            longRunning: false,
            serviceBinding: null,
            configuredEnabled: true,
            available: true,
            unavailableReason: null,
          },
        ],
      },
    })

    fireEvent.mouseDown(screen.getByRole('tab', { name: '工具' }))

    expect(await screen.findByText('文件系统')).toBeInTheDocument()
    expect(screen.queryByText('File system')).not.toBeInTheDocument()
  })

  it('falls back to backend labels for custom runtime tool groups', async () => {
    renderSkillSettingsPage({
      runtimeTools: {
        generation: 9,
        scope: 'project',
        customized: false,
        tools: [
          {
            name: 'custom_tool',
            displayName: 'Custom tool',
            description: 'Runs a custom workflow.',
            category: 'custom',
            group: 'teamWorkflow',
            groupLabel: 'Team workflow',
            originKind: 'custom',
            originId: null,
            access: 'mutating',
            executionChannel: 'directAuthorizedRust',
            requiredCapabilities: [],
            deferPolicy: 'alwaysLoad',
            longRunning: false,
            serviceBinding: null,
            configuredEnabled: true,
            available: true,
            unavailableReason: null,
          },
        ],
      },
    })

    fireEvent.mouseDown(screen.getByRole('tab', { name: '工具' }))

    expect(await screen.findByText('Team workflow')).toBeInTheDocument()
  })

  it('jumps from a plugin-provided skill to plugin details', async () => {
    renderSkillSettingsPage({
      skillSummaries: [
        {
          description: 'Formats workspace files.',
          enabled: true,
          id: 'format-file',
          manageable: false,
          name: 'format-file',
          sourceKind: 'plugin',
          sourcePluginId: 'formatter@1.0.0',
          status: 'ready',
          tags: ['formatting'],
        },
      ],
    })

    const card = await screen.findByText('format-file')
    fireEvent.click(
      (card.closest('[data-skill-card]') ?? document.body).querySelector(
        'button[aria-label="查看来源插件 formatter@1.0.0"]',
      ) as HTMLButtonElement,
    )

    expect(screen.getByRole('tab', { hidden: true, name: '插件' })).toHaveAttribute(
      'aria-selected',
      'true',
    )
    expect(await screen.findByText('/tmp/formatter-plugin/plugin.json')).toBeInTheDocument()
  })
})
