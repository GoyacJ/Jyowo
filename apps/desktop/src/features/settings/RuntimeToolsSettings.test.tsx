import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
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
})
