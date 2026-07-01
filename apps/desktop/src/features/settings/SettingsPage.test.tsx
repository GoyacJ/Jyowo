import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen } from '@testing-library/react'
import type { ReactNode } from 'react'
import { afterEach, describe, expect, it } from 'vitest'

import { AppI18nProvider } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'

import { SettingsPage } from './SettingsPage'

const emptyProviderSettingsList = {
  defaultConfigId: null,
  configs: [],
}

function renderSettingsPage() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  })

  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <CommandClientProvider
        client={createTestCommandClient({ providerSettingsList: emptyProviderSettingsList })}
      >
        <QueryClientProvider client={queryClient}>
          <AppI18nProvider>{children}</AppI18nProvider>
        </QueryClientProvider>
      </CommandClientProvider>
    )
  }

  return render(<SettingsPage />, { wrapper: Wrapper })
}

describe('SettingsPage', () => {
  afterEach(() => {
    uiStore.getState().setLocale('zh-CN')
  })

  it('switches the app language from local settings', () => {
    renderSettingsPage()

    expect(screen.getByRole('region', { name: '设置' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: '语言' })).toBeInTheDocument()

    fireEvent.change(screen.getByLabelText('应用语言'), { target: { value: 'en-US' } })

    expect(uiStore.getState().locale).toBe('en-US')
    expect(screen.getByRole('region', { name: 'Settings' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Language' })).toBeInTheDocument()
  })

  it('renders settings sections as top-level tabs', async () => {
    renderSettingsPage()

    expect(screen.getByRole('tab', { name: '通用' })).toHaveAttribute('aria-selected', 'true')
    expect(screen.getByRole('tab', { name: '技能' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: '工具' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: '自动化' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'MCP' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: '插件' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: '模型' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: '关于' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: '语言' })).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: '模型配置' })).not.toBeInTheDocument()

    fireEvent.mouseDown(screen.getByRole('tab', { name: '技能' }))

    expect(
      await screen.findByRole('button', { name: /Creates release notes from recent changes/ }),
    ).toBeInTheDocument()

    fireEvent.mouseDown(screen.getByRole('tab', { name: '工具' }))

    expect(screen.getByRole('heading', { name: '内置工具' })).toBeInTheDocument()

    fireEvent.mouseDown(screen.getByRole('tab', { name: '自动化' }))

    expect(await screen.findByRole('heading', { name: '自动化任务' })).toBeInTheDocument()

    fireEvent.mouseDown(screen.getByRole('tab', { name: 'MCP' }))

    expect(await screen.findByRole('heading', { name: 'MCP 服务器' })).toBeInTheDocument()

    fireEvent.mouseDown(screen.getByRole('tab', { name: '插件' }))

    expect(await screen.findByRole('heading', { name: '插件' })).toBeInTheDocument()

    fireEvent.mouseDown(screen.getByRole('tab', { name: '模型' }))

    expect(await screen.findByRole('heading', { name: '模型' })).toBeInTheDocument()
    expect(await screen.findByRole('heading', { name: '暂无已配置模型' })).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: '模型配置' })).not.toBeInTheDocument()

    fireEvent.mouseDown(screen.getByRole('tab', { name: '关于' }))

    expect(await screen.findByRole('heading', { name: '关于 Jyowo' })).toBeInTheDocument()
  })

  it('owns the right pane scroll container', () => {
    renderSettingsPage()

    expect(screen.getByRole('region', { name: '设置' })).toHaveClass('h-full')
    expect(screen.getByRole('region', { name: '设置' })).toHaveClass('overflow-y-auto')
  })
})
