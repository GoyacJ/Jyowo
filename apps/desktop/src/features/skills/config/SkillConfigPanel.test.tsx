import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'

import { SkillConfigPanel } from '@/features/skills/config/SkillConfigPanel'
import { AppI18nProvider } from '@/shared/i18n/i18n'
import type { CommandClient, GetSkillConfigResponse } from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'
import { fixtureSkillConfig } from '@/testing/command-client/skills'

function renderPanel(commandClient: CommandClient = createTestCommandClient()) {
  const queryClient = new QueryClient({
    defaultOptions: {
      mutations: { retry: false },
      queries: { retry: false },
    },
  })

  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <CommandClientProvider client={commandClient}>
        <QueryClientProvider client={queryClient}>
          <AppI18nProvider>{children}</AppI18nProvider>
        </QueryClientProvider>
      </CommandClientProvider>
    )
  }

  return {
    ...render(<SkillConfigPanel skillId={fixtureSkillConfig.skillId} />, { wrapper: Wrapper }),
    queryClient,
  }
}

function parentOf(element: HTMLElement): HTMLElement {
  const parent = element.parentElement
  if (!parent) throw new Error('expected element to have a parent')
  return parent
}

describe('SkillConfigPanel', () => {
  it('renders public values and secret presence without echoing a secret', async () => {
    renderPanel()

    const panel = await screen.findByRole('region', { name: '技能配置' })

    expect(within(panel).getByLabelText('region')).toHaveValue('cn-east')
    expect(within(panel).getByLabelText('retries')).toHaveValue(3)
    expect(within(panel).getByLabelText('useCache')).toBeChecked()
    expect(within(panel).getByLabelText('apiToken')).toHaveAttribute('type', 'password')
    expect(within(panel).getByLabelText('apiToken')).toHaveValue('')
    expect(within(panel).getByText('已配置')).toBeInTheDocument()
  })

  it('saves typed public values and invalidates config, detail, and list queries', async () => {
    const baseClient = createTestCommandClient()
    const setSkillConfigValue = vi.fn(baseClient.setSkillConfigValue)
    const commandClient = { ...baseClient, setSkillConfigValue }
    const { queryClient } = renderPanel(commandClient)
    const invalidateQueries = vi.spyOn(queryClient, 'invalidateQueries')

    const regionInput = await screen.findByLabelText('region')
    fireEvent.change(regionInput, { target: { value: 'us-west' } })
    fireEvent.click(within(parentOf(regionInput)).getByRole('button', { name: '保存' }))

    await waitFor(() => {
      expect(setSkillConfigValue).toHaveBeenCalledWith(
        fixtureSkillConfig.skillId,
        'region',
        'us-west',
      )
    })
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: ['skills', 'config', fixtureSkillConfig.skillId],
    })
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: ['skills', 'detail', fixtureSkillConfig.skillId],
    })
    expect(invalidateQueries).toHaveBeenCalledWith({ queryKey: ['skills', 'list'] })

    const retriesInput = screen.getByLabelText('retries')
    fireEvent.change(retriesInput, { target: { value: '5' } })
    fireEvent.click(within(parentOf(retriesInput)).getByRole('button', { name: '保存' }))

    await waitFor(() => {
      expect(setSkillConfigValue).toHaveBeenCalledWith(fixtureSkillConfig.skillId, 'retries', 5)
    })

    fireEvent.click(screen.getByLabelText('useCache'))
    const booleanSaveButton = within(parentOf(screen.getByLabelText('useCache'))).getByRole(
      'button',
      { name: '保存' },
    )
    fireEvent.click(booleanSaveButton)

    await waitFor(() => {
      expect(setSkillConfigValue).toHaveBeenCalledWith(
        fixtureSkillConfig.skillId,
        'useCache',
        false,
      )
    })
  })

  it('sets, replaces, and clears secrets without retaining the submitted value', async () => {
    const unconfigured: GetSkillConfigResponse = {
      ...fixtureSkillConfig,
      config: {
        ...fixtureSkillConfig.config,
        secrets: { apiToken: { configured: false } },
      },
    }
    const baseClient = createTestCommandClient({ skillConfig: unconfigured })
    const setSkillSecret = vi.fn(baseClient.setSkillSecret)
    const clearSkillSecret = vi.fn(baseClient.clearSkillSecret)
    const { queryClient } = renderPanel({ ...baseClient, clearSkillSecret, setSkillSecret })

    const secretInput = await screen.findByLabelText('apiToken')
    expect(screen.getByText('未配置')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '清除' })).not.toBeInTheDocument()

    fireEvent.change(secretInput, { target: { value: 'new-secret-value' } })
    fireEvent.click(screen.getByRole('button', { name: '设置' }))

    await waitFor(() => {
      expect(setSkillSecret).toHaveBeenCalledWith(
        fixtureSkillConfig.skillId,
        'apiToken',
        'new-secret-value',
      )
      expect(secretInput).toHaveValue('')
      expect(screen.getByText('已配置')).toBeInTheDocument()
    })
    expect(screen.queryByDisplayValue('new-secret-value')).not.toBeInTheDocument()
    expect(JSON.stringify(queryClient.getMutationCache().getAll())).not.toContain(
      'new-secret-value',
    )

    fireEvent.change(secretInput, { target: { value: 'replacement-secret' } })
    fireEvent.click(screen.getByRole('button', { name: '替换' }))

    await waitFor(() => {
      expect(setSkillSecret).toHaveBeenLastCalledWith(
        fixtureSkillConfig.skillId,
        'apiToken',
        'replacement-secret',
      )
      expect(secretInput).toHaveValue('')
    })

    fireEvent.click(screen.getByRole('button', { name: '清除' }))

    await waitFor(() => {
      expect(clearSkillSecret).toHaveBeenCalledWith(fixtureSkillConfig.skillId, 'apiToken')
      expect(screen.getByText('未配置')).toBeInTheDocument()
    })
    expect(screen.queryByRole('button', { name: '清除' })).not.toBeInTheDocument()
  })

  it.each([
    ['public save', 'region', '保存', 'setSkillConfigValue'],
    ['secret set', 'apiToken', '替换', 'setSkillSecret'],
    ['secret clear', null, '清除', 'clearSkillSecret'],
  ] as const)('surfaces the command error when %s is rejected', async (_name, field, action, method) => {
    const baseClient = createTestCommandClient()
    const commandClient = {
      ...baseClient,
      [method]: vi.fn().mockRejectedValue(new Error(`${method} rejected`)),
    } as CommandClient
    renderPanel(commandClient)

    let actionButton: HTMLElement
    if (field) {
      const input = await screen.findByLabelText(field)
      if (method === 'setSkillSecret') {
        fireEvent.change(input, { target: { value: 'not-rendered-after-rejection' } })
        actionButton = screen.getByRole('button', { name: action })
      } else {
        actionButton = within(parentOf(input)).getByRole('button', { name: action })
      }
    } else {
      await screen.findByLabelText('apiToken')
      actionButton = screen.getByRole('button', { name: action })
    }
    fireEvent.click(actionButton)

    expect(await screen.findByRole('alert')).toHaveTextContent(`${method} rejected`)
  })

  it('surfaces configuration read errors', async () => {
    const baseClient = createTestCommandClient()
    renderPanel({
      ...baseClient,
      getSkillConfig: vi.fn().mockRejectedValue(new Error('config read rejected')),
    })

    expect(await screen.findByText('技能配置无法加载。')).toBeInTheDocument()
    expect(screen.getByRole('alert')).toHaveTextContent('config read rejected')
  })
})
