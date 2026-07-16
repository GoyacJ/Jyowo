import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'

import { AppI18nProvider } from '@/shared/i18n/i18n'
import type { ListRuntimeToolsResponse } from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'
import { fixtureRuntimeTools } from '@/testing/command-client/base'

import { RuntimeToolsSettings } from './RuntimeToolsSettings'

function renderTools(runtimeTools: ListRuntimeToolsResponse = fixtureRuntimeTools) {
  const client = createTestCommandClient({ runtimeTools })
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })

  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <CommandClientProvider client={client}>
        <QueryClientProvider client={queryClient}>
          <AppI18nProvider>{children}</AppI18nProvider>
        </QueryClientProvider>
      </CommandClientProvider>
    )
  }

  return { client, ...render(<RuntimeToolsSettings />, { wrapper: Wrapper }) }
}

describe('RuntimeToolsSettings', () => {
  it('renders a grouped list with runtime summary and no wide table', async () => {
    const { container } = renderTools()

    expect(await screen.findByRole('heading', { name: '工具' })).toBeInTheDocument()
    expect(await screen.findByText('当前工作区')).toBeInTheDocument()
    expect(screen.getByText('已启用 2 / 2')).toBeInTheDocument()
    expect(screen.getByText('文件系统')).toBeInTheDocument()
    expect(screen.getByText('Shell')).toBeInTheDocument()
    expect(screen.getByText('沙箱 routing')).toBeInTheDocument()
    expect(container.querySelector('table')).not.toBeInTheDocument()
  })

  it('persists a normal switch change and updates its state', async () => {
    const { client } = renderTools()
    const setTool = vi.spyOn(client, 'setRuntimeToolEnabled')

    fireEvent.click(await screen.findByRole('switch', { name: 'File read 开关' }))

    await waitFor(() => {
      expect(setTool).toHaveBeenCalledWith({ enabled: false, name: 'FileRead' })
    })
    expect(await screen.findByText('已停用')).toBeInTheDocument()
    expect(screen.getByText('已启用 1 / 2')).toBeInTheDocument()
  })

  it('requires confirmation before enabling a destructive tool', async () => {
    const runtimeTools: ListRuntimeToolsResponse = {
      ...fixtureRuntimeTools,
      tools: fixtureRuntimeTools.tools.map((tool) =>
        tool.name === 'Bash' ? { ...tool, configuredEnabled: false } : tool,
      ),
    }
    const { client } = renderTools(runtimeTools)
    const setTool = vi.spyOn(client, 'setRuntimeToolEnabled')

    fireEvent.click(await screen.findByRole('switch', { name: 'Bash 开关' }))

    expect(screen.getByRole('dialog')).toBeInTheDocument()
    expect(setTool).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: '确认启用' }))

    await waitFor(() => {
      expect(setTool).toHaveBeenCalledWith({ enabled: true, name: 'Bash' })
    })
  })

  it('filters enabled, unavailable, and high-risk tools without changing switch state', async () => {
    const runtimeTools: ListRuntimeToolsResponse = {
      ...fixtureRuntimeTools,
      tools: fixtureRuntimeTools.tools.map((tool) =>
        tool.name === 'Bash'
          ? {
              ...tool,
              available: false,
              unavailableReason: 'Process sandbox unavailable',
            }
          : tool,
      ),
    }
    renderTools(runtimeTools)

    fireEvent.click(await screen.findByRole('button', { name: '不可用' }))
    expect(screen.getByText('Bash')).toBeInTheDocument()
    expect(screen.queryByText('File read')).not.toBeInTheDocument()
    expect(screen.getByRole('switch', { name: 'Bash 开关' })).toBeChecked()

    fireEvent.click(screen.getByRole('button', { name: '高风险' }))
    expect(screen.getByText('Bash')).toBeInTheDocument()
    expect(screen.queryByText('File read')).not.toBeInTheDocument()
  })

  it('collapses and expands each tool group independently', async () => {
    renderTools()

    const fileSystemGroup = await screen.findByRole('button', { name: /文件系统/ })
    const shellGroup = screen.getByRole('button', { name: /Shell/ })
    expect(fileSystemGroup).toHaveAttribute('aria-expanded', 'true')
    expect(shellGroup).toHaveAttribute('aria-expanded', 'true')

    fireEvent.click(fileSystemGroup)

    expect(fileSystemGroup).toHaveAttribute('aria-expanded', 'false')
    expect(screen.getByText('File read')).not.toBeVisible()
    expect(screen.getByText('Bash')).toBeVisible()

    fireEvent.click(fileSystemGroup)
    expect(screen.getByText('File read')).toBeVisible()
  })

  it('saves schema-backed parameters and timeout, then resets the tool configuration', async () => {
    const webFetch = {
      ...fixtureRuntimeTools.tools[0],
      name: 'WebFetch',
      displayName: 'Web fetch',
      description: 'Fetch text content from an HTTP URL.',
      group: 'network',
      groupLabel: 'Network',
      executionChannel: 'httpBroker' as const,
      configurationSchema: {
        type: 'object',
        properties: {
          defaultMaxBytes: {
            type: 'integer',
            minimum: 1,
            maximum: 10_485_760,
          },
        },
        additionalProperties: false,
      },
      defaultParameters: { defaultMaxBytes: 64_000 },
      parameters: { defaultMaxBytes: 64_000 },
    }
    const { client } = renderTools({
      ...fixtureRuntimeTools,
      tools: [webFetch],
    })
    const updateConfig = vi.spyOn(client, 'updateRuntimeToolConfig')
    const resetConfig = vi.spyOn(client, 'resetRuntimeToolConfig')

    fireEvent.click(await screen.findByRole('button', { name: '配置 Web fetch' }))
    const dialog = screen.getByRole('dialog', { name: '配置 Web fetch' })
    fireEvent.change(within(dialog).getByRole('spinbutton', { name: /执行超时/ }), {
      target: { value: '45' },
    })
    fireEvent.change(within(dialog).getByRole('spinbutton', { name: /默认响应上限/ }), {
      target: { value: '128000' },
    })
    fireEvent.click(within(dialog).getByRole('button', { name: '保存配置' }))

    await waitFor(() => {
      expect(updateConfig).toHaveBeenCalledWith({
        name: 'WebFetch',
        timeoutMs: 45_000,
        parameters: { defaultMaxBytes: 128_000 },
      })
    })
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())

    fireEvent.click(screen.getByRole('button', { name: '配置 Web fetch' }))
    fireEvent.click(
      screen.getByRole('button', {
        name: '恢复此工具默认值',
      }),
    )

    await waitFor(() => {
      expect(resetConfig).toHaveBeenCalledWith({ name: 'WebFetch' })
    })
  })
})
