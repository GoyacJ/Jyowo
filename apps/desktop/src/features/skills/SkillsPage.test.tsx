import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { SkillSettingsPage } from '@/features/settings/SkillSettings'
import { AppI18nProvider } from '@/shared/i18n/i18n'
import type {
  CommandClient,
  GetSkillCatalogEntryResponse,
  InstallSkillFromCatalogResponse,
  ListSkillsResponse,
  SkillCatalogInstallProgressPayload,
} from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createRejectedTestCommandClient, createTestCommandClient } from '@/testing/command-client'

const openDialogSpy = vi.hoisted(() => vi.fn())
const routerSpy = vi.hoisted(() => ({
  navigate: vi.fn(async ({ search, to }: { search?: Record<string, string>; to: string }) => {
    const nextSearch = search ? `?${new URLSearchParams(search).toString()}` : ''
    window.history.pushState(null, '', `${to}${nextSearch}`)
  }),
}))

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: openDialogSpy,
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

function renderSkillsPage(commandClient: CommandClient = createTestCommandClient()) {
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
    openDialogSpy.mockReset()
    routerSpy.navigate.mockClear()
    window.history.replaceState(null, '', '/skills')
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
    renderSkillsPage(createTestCommandClient({ delayMs: 50 }))

    expect(screen.getByText('正在加载技能。')).toBeInTheDocument()

    renderSkillsPage(createTestCommandClient({ skills: { skills: [] } }))

    expect(await screen.findByText('暂无技能配置。')).toBeInTheDocument()

    renderSkillsPage(createRejectedTestCommandClient(new Error('raw secret path')))

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
      await within(detail).findByText('Fixture content for references/style.md'),
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

    renderSkillsPage(createTestCommandClient({ skills }))

    expect(await screen.findByRole('button', { name: /Skill package 01/ })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Skill package 09/ })).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: '下一页' }))

    expect(await screen.findByRole('button', { name: /Skill package 09/ })).toBeInTheDocument()
    expect(screen.getByText('2 / 2')).toBeInTheDocument()
  })

  it('imports a skill package through the system directory picker', async () => {
    openDialogSpy.mockResolvedValue('/tmp/release-notes')
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
      ...createTestCommandClient({ skills: { skills: [] } }),
      importSkill,
    }

    renderSkillsPage(client)

    fireEvent.click(screen.getByRole('button', { name: '导入' }))

    await waitFor(() =>
      expect(openDialogSpy).toHaveBeenCalledWith({ directory: true, multiple: false }),
    )
    await waitFor(() => expect(importSkill).toHaveBeenCalledWith('/tmp/release-notes'))
  })

  it('installs a skill from the catalog', async () => {
    const installSkillFromCatalog = vi.fn().mockResolvedValue({
      task: {
        entryId: 'anthropic:frontend-design',
        operationId: 'catalog-install-001',
        percent: 5,
        sourceId: 'anthropic',
        stage: 'preparing',
        startedAt: '2026-06-28T00:00:00Z',
        status: 'running',
        updatedAt: '2026-06-28T00:00:00Z',
        version: 'main',
      },
    })
    const client = {
      ...createTestCommandClient(),
      installSkillFromCatalog,
    }

    renderSkillsPage(client)

    fireEvent.mouseDown(screen.getByRole('tab', { name: '技能目录' }))

    expect(await screen.findByRole('button', { name: /frontend-design/ })).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /frontend-design/ }))
    const dialog = await screen.findByRole('dialog')
    expect(within(dialog).getByRole('heading', { name: 'frontend-design' })).toBeInTheDocument()
    await waitFor(() => expect(within(dialog).getByRole('button', { name: '安装' })).toBeEnabled())
    fireEvent.click(within(dialog).getByRole('button', { name: '安装' }))

    await waitFor(() =>
      expect(installSkillFromCatalog).toHaveBeenCalledWith({
        entryId: 'anthropic:frontend-design',
        operationId: expect.stringMatching(/^catalog-install-/),
        sourceId: 'anthropic',
        version: 'main',
      }),
    )
  })

  it('shows running catalog install progress on the catalog entry card', async () => {
    const client = createTestCommandClient({
      skillCatalogInstallTasks: {
        tasks: [
          {
            entryId: 'anthropic:frontend-design',
            operationId: 'catalog-install-001',
            percent: 45,
            sourceId: 'anthropic',
            stage: 'downloading',
            startedAt: '2026-06-28T00:00:00Z',
            status: 'running',
            updatedAt: '2026-06-28T00:00:01Z',
            version: 'main',
          },
        ],
      },
    })

    renderSkillsPage(client)

    fireEvent.mouseDown(screen.getByRole('tab', { name: '技能目录' }))

    const entryCard = await screen.findByRole('button', { name: /frontend-design/ })
    expect(within(entryCard).getByText('45%')).toBeInTheDocument()
  })

  it('shows the backend catalog install error message', async () => {
    const installSkillFromCatalog = vi
      .fn()
      .mockRejectedValue({ code: 'missing_skill_file', message: '缺少 SKILL.md。' })
    const client = {
      ...createTestCommandClient(),
      installSkillFromCatalog,
    }

    renderSkillsPage(client)

    fireEvent.mouseDown(screen.getByRole('tab', { name: '技能目录' }))
    fireEvent.click(await screen.findByRole('button', { name: /frontend-design/ }))
    const dialog = await screen.findByRole('dialog')
    await waitFor(() => expect(within(dialog).getByRole('button', { name: '安装' })).toBeEnabled())
    fireEvent.click(within(dialog).getByRole('button', { name: '安装' }))

    expect(await within(dialog).findByText('缺少 SKILL.md。')).toBeInTheDocument()
    expect(within(dialog).queryByText('目录技能无法安装。')).not.toBeInTheDocument()
  })

  it('shows real catalog install progress inside the install button', async () => {
    let progressListener: (progress: SkillCatalogInstallProgressPayload) => void = (_progress) => {
      throw new Error('catalog install progress listener was not registered')
    }
    let resolveInstall: () => void = () => {
      throw new Error('catalog install resolver was not registered')
    }
    const installSkillFromCatalog = vi.fn<CommandClient['installSkillFromCatalog']>(
      () =>
        new Promise<InstallSkillFromCatalogResponse>((resolve) => {
          resolveInstall = () =>
            resolve({
              task: {
                entryId: 'anthropic:frontend-design',
                operationId: 'catalog-install-001',
                percent: 5,
                sourceId: 'anthropic',
                stage: 'preparing',
                startedAt: '2026-06-28T00:00:00Z',
                status: 'running',
                updatedAt: '2026-06-28T00:00:00Z',
                version: 'main',
              },
            } satisfies InstallSkillFromCatalogResponse)
        }),
    )
    const unlisten = vi.fn()
    const listenSkillCatalogInstallProgress = vi.fn(
      async (listener: (progress: SkillCatalogInstallProgressPayload) => void) => {
        progressListener = listener
        return unlisten
      },
    )
    const client = {
      ...createTestCommandClient(),
      installSkillFromCatalog,
      listenSkillCatalogInstallProgress,
    }

    const rendered = renderSkillsPage(client)

    fireEvent.mouseDown(screen.getByRole('tab', { name: '技能目录' }))
    fireEvent.click(await screen.findByRole('button', { name: /frontend-design/ }))
    const dialog = await screen.findByRole('dialog')
    await waitFor(() => expect(within(dialog).getByRole('button', { name: '安装' })).toBeEnabled())
    fireEvent.click(within(dialog).getByRole('button', { name: '安装' }))

    await waitFor(() => expect(listenSkillCatalogInstallProgress).toHaveBeenCalledTimes(1))
    const operationId = installSkillFromCatalog.mock.calls[0]?.[0]?.operationId
    expect(operationId).toEqual(expect.stringMatching(/^catalog-install-/))
    if (typeof operationId !== 'string') {
      throw new Error('expected catalog install operation id')
    }
    progressListener({
      entryId: 'anthropic:frontend-design',
      operationId,
      percent: 45,
      sourceId: 'anthropic',
      stage: 'downloading',
      version: 'main',
    })

    expect(await within(dialog).findByRole('button', { name: /正在下载 45%/ })).toBeDisabled()
    resolveInstall()
    await waitFor(() => expect(installSkillFromCatalog).toHaveBeenCalledTimes(1))
    expect(unlisten).not.toHaveBeenCalled()

    rendered.unmount()
    await waitFor(() => expect(unlisten).toHaveBeenCalledTimes(1))
  })

  it('shows blocked catalog validation instead of a detail load error', async () => {
    const blockedDetail: GetSkillCatalogEntryResponse = {
      entry: {
        description: 'Community entry without a skill package.',
        entryId: 'awesome:missing-skill',
        installable: false,
        installed: false,
        name: 'missing-skill',
        sourceId: 'awesome-agent-skills',
        sourceLabel: 'Awesome Agent Skills',
        tags: [],
        trustLevel: 'curated',
        version: 'main',
      },
      files: [{ kind: 'file', path: 'README.md', sizeBytes: 128 }],
      readmePreview: 'Community entry without a SKILL.md file.',
      validation: {
        issueCodes: ['missing_skill_file'],
        issues: ['缺少 SKILL.md。'],
        status: 'blocked',
      },
    }
    const client = createTestCommandClient({
      skillCatalogEntries: { entries: [blockedDetail.entry] },
      skillCatalogEntry: blockedDetail,
    })

    renderSkillsPage(client)

    fireEvent.mouseDown(screen.getByRole('tab', { name: '技能目录' }))
    fireEvent.click(await screen.findByRole('button', { name: 'missing-skill' }))
    const dialog = await screen.findByRole('dialog')

    expect(await within(dialog).findByText('缺少 SKILL.md。')).toBeInTheDocument()
    expect(within(dialog).queryByText('目录详情无法加载。')).not.toBeInTheDocument()
    expect(within(dialog).queryByRole('button', { name: '安装' })).not.toBeInTheDocument()
  })

  it('previews catalog files and switches content from the file tree', async () => {
    const detail: GetSkillCatalogEntryResponse = {
      entry: {
        description: 'Create distinctive frontend interfaces.',
        entryId: 'anthropic:frontend-design',
        installable: true,
        installed: false,
        name: 'frontend-design',
        sourceId: 'anthropic',
        sourceLabel: 'Anthropic Skills',
        tags: ['frontend'],
        trustLevel: 'official',
        version: 'main',
      },
      files: [
        { kind: 'file', path: 'README.md', sizeBytes: 96 },
        { kind: 'file', path: 'SKILL.md', sizeBytes: 128 },
        { kind: 'directory', path: 'references' },
        { kind: 'file', path: 'references/style.md', sizeBytes: 64 },
      ],
      validation: {
        issues: [],
        status: 'ready',
      },
    }
    const getSkillCatalogFile = vi.fn(({ path }: { path: string }) =>
      Promise.resolve({
        file: {
          content: `Catalog content for ${path}`,
          path,
          truncated: path === 'references/style.md',
        },
      }),
    )
    const client = {
      ...createTestCommandClient({ skillCatalogEntry: detail }),
      getSkillCatalogFile,
    }

    renderSkillsPage(client)

    fireEvent.mouseDown(screen.getByRole('tab', { name: '技能目录' }))
    fireEvent.click(await screen.findByRole('button', { name: /frontend-design/ }))
    const dialog = await screen.findByRole('dialog')

    expect(await within(dialog).findByRole('button', { name: /SKILL.md/ })).toBeInTheDocument()
    expect(await within(dialog).findByText('Catalog content for SKILL.md')).toBeInTheDocument()
    expect(getSkillCatalogFile).toHaveBeenCalledWith({
      entryId: 'anthropic:frontend-design',
      path: 'SKILL.md',
      sourceId: 'anthropic',
      version: 'main',
    })

    fireEvent.click(within(dialog).getByRole('button', { name: /style.md/ }))

    expect(
      await within(dialog).findByText('Catalog content for references/style.md'),
    ).toBeInTheDocument()
    expect(within(dialog).getByText('文件内容已截断。')).toBeInTheDocument()
  })

  it('disables catalog installation when validation is blocked', async () => {
    const blockedDetail: GetSkillCatalogEntryResponse = {
      entry: {
        description: 'Name already exists.',
        entryId: 'anthropic:frontend-design',
        installable: true,
        installed: false,
        name: 'frontend-design',
        sourceId: 'anthropic',
        sourceLabel: 'Anthropic Skills',
        tags: [],
        trustLevel: 'official',
        version: 'main',
      },
      files: [{ kind: 'file', path: 'SKILL.md' }],
      validation: {
        issueCodes: ['active_skill_name_exists'],
        issues: ['同名技能已存在。'],
        status: 'blocked',
      },
    }
    const installSkillFromCatalog = vi.fn()
    const client = {
      ...createTestCommandClient({ skillCatalogEntry: blockedDetail }),
      installSkillFromCatalog,
    }

    renderSkillsPage(client)

    fireEvent.mouseDown(screen.getByRole('tab', { name: '技能目录' }))
    fireEvent.click(await screen.findByRole('button', { name: /frontend-design/ }))
    const dialog = await screen.findByRole('dialog')

    expect(await within(dialog).findByText('同名技能已存在。')).toBeInTheDocument()
    expect(within(dialog).getByRole('button', { name: '安装' })).toBeDisabled()
  })

  it('localizes catalog sources and paginates catalog entries', async () => {
    const listSkillCatalogEntries = vi
      .fn()
      .mockResolvedValueOnce({
        entries: Array.from({ length: 12 }, (_, index) => ({
          description: `Catalog skill ${index + 1}`,
          entryId: `anthropic:skill-${index + 1}`,
          installable: true,
          installed: false,
          name: `skill-${index + 1}`,
          sourceId: 'anthropic',
          sourceLabel: 'Anthropic Skills',
          tags: [],
          trustLevel: 'official',
          version: 'main',
        })),
        nextCursor: 'offset:12',
      })
      .mockResolvedValueOnce({
        entries: [
          {
            description: 'Catalog skill 13',
            entryId: 'anthropic:skill-13',
            installable: true,
            installed: false,
            name: 'skill-13',
            sourceId: 'anthropic',
            sourceLabel: 'Anthropic Skills',
            tags: [],
            trustLevel: 'official',
            version: 'main',
          },
        ],
      })
    const client = {
      ...createTestCommandClient(),
      listSkillCatalogEntries,
    }

    renderSkillsPage(client)

    fireEvent.mouseDown(screen.getByRole('tab', { name: '技能目录' }))

    expect(
      await screen.findByRole('button', { name: /官方 Anthropic 技能仓库/ }),
    ).toBeInTheDocument()
    expect(await screen.findByRole('button', { name: 'skill-1' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'skill-13' })).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: '下一页' }))

    expect(await screen.findByRole('button', { name: 'skill-13' })).toBeInTheDocument()
    expect(listSkillCatalogEntries).toHaveBeenLastCalledWith({
      cursor: 'offset:12',
      limit: 12,
      sourceId: 'anthropic',
    })
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
      ...createTestCommandClient({ skills }),
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
