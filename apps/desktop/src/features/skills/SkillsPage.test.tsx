import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { SkillSettingsPage } from '@/features/settings/SkillSettings'
import { AppI18nProvider } from '@/shared/i18n/i18n'
import type { CommandClient, ListSkillsResponse } from '@/shared/tauri/commands'
import { createMockCommandClient, createRejectedCommandClient } from '@/shared/tauri/mock-client'
import { CommandClientProvider } from '@/shared/tauri/react'

const openMock = vi.hoisted(() => vi.fn())

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: openMock,
}))

function renderSkillsPage(commandClient: CommandClient = createMockCommandClient()) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
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

  return render(<SkillSettingsPage />, { wrapper: Wrapper })
}

describe('SkillsPage', () => {
  beforeEach(() => {
    openMock.mockReset()
  })

  it('renders skills, tools, and MCP sections as tabs', async () => {
    renderSkillsPage()

    expect(screen.getByRole('region', { name: '技能' })).toHaveClass('overflow-y-auto')
    expect(screen.queryByRole('heading', { level: 1, name: '技能' })).not.toBeInTheDocument()
    expect(screen.queryByRole('heading', { level: 2, name: '技能' })).not.toBeInTheDocument()
    expect(screen.getByRole('tab', { name: '技能' })).toHaveAttribute('aria-selected', 'true')
    expect(screen.getByRole('tab', { name: '工具' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'MCP' })).toBeInTheDocument()
    expect(
      await screen.findByRole('button', { name: /Creates release notes from recent changes/ }),
    ).toBeInTheDocument()

    fireEvent.mouseDown(screen.getByRole('tab', { name: '工具' }))

    expect(screen.getByRole('heading', { name: '内置工具' })).toBeInTheDocument()
    expect(screen.getByText('FileRead')).toBeInTheDocument()
    expect(screen.getByText('Bash')).toBeInTheDocument()
    expect(screen.getByText('skills_invoke')).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: 'Provider 设置' })).not.toBeInTheDocument()

    fireEvent.mouseDown(screen.getByRole('tab', { name: 'MCP' }))

    expect(await screen.findByRole('heading', { name: 'MCP 服务器' })).toBeInTheDocument()
  })

  it('renders loading, empty, and error states for skill list', async () => {
    renderSkillsPage(createMockCommandClient({ delayMs: 50 }))

    expect(screen.getByText('正在加载技能。')).toBeInTheDocument()

    renderSkillsPage(createMockCommandClient({ skills: { skills: [] } }))

    expect(await screen.findByText('暂无技能配置。')).toBeInTheDocument()

    renderSkillsPage(createRejectedCommandClient(new Error('raw secret path')))

    expect(await screen.findByText('技能无法加载。')).toBeInTheDocument()
    expect(screen.queryByText(/raw secret path/)).not.toBeInTheDocument()
  })

  it('shows skill detail after selecting a skill', async () => {
    renderSkillsPage()

    fireEvent.click(
      await screen.findByRole('button', { name: /Creates release notes from recent changes/ }),
    )

    const detail = await screen.findByRole('region', { name: '技能详情' })

    expect(within(detail).getByRole('heading', { name: 'release-notes' })).toBeInTheDocument()
    expect(
      within(detail).getByText('Creates release notes from recent changes.'),
    ).toBeInTheDocument()
    expect(within(detail).getByRole('tab', { name: '概览' })).toHaveAttribute(
      'aria-selected',
      'true',
    )
    expect(within(detail).getByRole('tab', { name: '文件' })).toBeInTheDocument()
    expect(within(detail).getByRole('tab', { name: '参数' })).toBeInTheDocument()
    expect(within(detail).getByRole('tab', { name: '配置' })).toBeInTheDocument()
    expect(screen.queryByRole('region', { name: '文件' })).not.toBeInTheDocument()

    fireEvent.mouseDown(within(detail).getByRole('tab', { name: '文件' }))

    expect(screen.getByRole('region', { name: '文件' })).toBeInTheDocument()
    expect(await screen.findByRole('button', { name: /SKILL.md/ })).toBeInTheDocument()
    expect(await within(detail).findByText(/Write concise release notes/)).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: /style.md/ }))

    expect(
      await within(detail).findByText('Mock content for references/style.md'),
    ).toBeInTheDocument()

    fireEvent.mouseDown(within(detail).getByRole('tab', { name: '参数' }))

    expect(await within(detail).findByText('version')).toBeInTheDocument()

    fireEvent.mouseDown(within(detail).getByRole('tab', { name: '配置' }))

    expect(await within(detail).findByText('CHANGELOG_TOKEN')).toBeInTheDocument()
  })

  it('paginates the skill list', async () => {
    const skills: ListSkillsResponse = {
      skills: Array.from({ length: 10 }, (_, index) => {
        const number = String(index + 1).padStart(2, '0')

        return {
          description: `Skill package ${number}`,
          enabled: true,
          id: `skill-${number}`,
          manageable: true,
          name: `skill-${number}`,
          sourceKind: 'workspace',
          status: 'ready',
          tags: [],
        }
      }),
    }

    renderSkillsPage(createMockCommandClient({ skills }))

    expect(await screen.findByRole('button', { name: /Skill package 01/ })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Skill package 09/ })).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: '下一页' }))

    expect(await screen.findByRole('button', { name: /Skill package 09/ })).toBeInTheDocument()
    expect(screen.getByText('2 / 2')).toBeInTheDocument()
  })

  it('imports a skill package through the system directory picker', async () => {
    openMock.mockResolvedValue('/tmp/release-notes')
    const importSkill = vi.fn().mockResolvedValue({
      skill: {
        description: 'Imported skill',
        enabled: true,
        id: 'skill-imported',
        manageable: true,
        name: 'imported-skill',
        sourceKind: 'workspace',
        status: 'ready',
        tags: [],
      },
    })
    const client = {
      ...createMockCommandClient({ skills: { skills: [] } }),
      importSkill,
    }

    renderSkillsPage(client)

    fireEvent.click(screen.getByRole('button', { name: '导入' }))

    await waitFor(() => expect(openMock).toHaveBeenCalledWith({ directory: true, multiple: false }))
    await waitFor(() => expect(importSkill).toHaveBeenCalledWith('/tmp/release-notes'))
  })

  it('installs a skill from the catalog', async () => {
    const installSkillFromCatalog = vi.fn().mockResolvedValue({
      skill: {
        description: 'Create distinctive frontend interfaces.',
        enabled: true,
        id: 'skill-catalog-001',
        manageable: true,
        name: 'frontend-design',
        origin: {
          entryId: 'anthropic:frontend-design',
          installedFromCatalog: true,
          sourceId: 'anthropic',
          sourceLabel: 'Anthropic Skills',
          version: 'main',
        },
        sourceKind: 'workspace',
        status: 'ready',
        tags: ['frontend'],
      },
    })
    const client = {
      ...createMockCommandClient(),
      installSkillFromCatalog,
    }

    renderSkillsPage(client)

    fireEvent.mouseDown(screen.getByRole('tab', { name: 'Catalog' }))

    expect(await screen.findByRole('button', { name: /frontend-design/ })).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /frontend-design/ }))
    fireEvent.click(await screen.findByRole('button', { name: '安装' }))

    await waitFor(() =>
      expect(installSkillFromCatalog).toHaveBeenCalledWith({
        entryId: 'anthropic:frontend-design',
        sourceId: 'anthropic',
        version: 'main',
      }),
    )
  })

  it('allows workspace skills to be disabled, enabled, and deleted', async () => {
    const skills: ListSkillsResponse = {
      skills: [
        {
          description: 'Creates release notes from recent changes.',
          enabled: true,
          id: 'skill-001',
          manageable: true,
          name: 'release-notes',
          sourceKind: 'workspace',
          status: 'ready',
          tags: ['writing'],
        },
        {
          description: 'Disabled workspace skill.',
          enabled: false,
          id: 'skill-002',
          manageable: true,
          name: 'disabled-skill',
          sourceKind: 'workspace',
          status: 'disabled',
          tags: [],
        },
      ],
    }
    const setSkillEnabled = vi.fn().mockResolvedValue({ skill: skills.skills[0] })
    const deleteSkill = vi.fn().mockResolvedValue({ id: 'skill-001', status: 'deleted' })
    const client = {
      ...createMockCommandClient({ skills }),
      deleteSkill,
      setSkillEnabled,
    }

    renderSkillsPage(client)

    fireEvent.click(await screen.findByRole('switch', { name: '停用 release-notes' }))

    await waitFor(() => expect(setSkillEnabled).toHaveBeenCalledWith('skill-001', false))

    fireEvent.click(await screen.findByRole('switch', { name: '启用 disabled-skill' }))

    await waitFor(() => expect(setSkillEnabled).toHaveBeenCalledWith('skill-002', true))

    fireEvent.click(await screen.findByRole('button', { name: '删除 release-notes' }))
    fireEvent.click(await screen.findByRole('button', { name: '确认删除' }))

    await waitFor(() => expect(deleteSkill).toHaveBeenCalledWith('skill-001'))
  })

  it('keeps non-manageable skills read-only', async () => {
    renderSkillsPage()

    fireEvent.click(await screen.findByRole('button', { name: /Inspects source changes/ }))

    const detail = await screen.findByRole('region', { name: '技能详情' })

    expect(within(detail).getByText('只读')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /删除 code-review/ })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /停用 code-review/ })).not.toBeInTheDocument()
  })
})
