import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate, useRouterState } from '@tanstack/react-router'
import {
  ChevronLeft,
  ChevronRight,
  Download,
  ExternalLink,
  FileText,
  Folder,
  Search,
  Trash2,
  Upload,
  Wrench,
} from 'lucide-react'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  deleteSkill,
  type GetSkillCatalogFileResponse,
  getSkillCatalogEntry,
  getSkillCatalogFile,
  getSkillDetail,
  getSkillFile,
  importSkill,
  installSkillFromCatalog,
  listenSkillCatalogInstallProgress,
  listRuntimeTools,
  listSkillCatalogEntries,
  listSkillCatalogInstallTasks,
  listSkillCatalogSources,
  listSkills,
  type RuntimeToolSummary,
  type SkillCatalogEntry,
  type SkillCatalogInstallProgressPayload,
  type SkillCatalogInstallTask,
  type SkillCatalogSource,
  type SkillFile,
  type SkillSummary,
  setSkillEnabled,
} from '@/shared/tauri/commands'
import { pickSkillPackagePath } from '@/shared/tauri/file-dialog'
import { useCommandClient } from '@/shared/tauri/react'
import { Badge } from '@/shared/ui/badge'
import { Button } from '@/shared/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shared/ui/dialog'
import { Input } from '@/shared/ui/input'
import { Select } from '@/shared/ui/select'
import { Switch } from '@/shared/ui/switch'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/shared/ui/tabs'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/shared/ui/tooltip'

import { MCPManager } from './MCPManager'
import { type PluginOpenRequest, PluginsManager } from './PluginsManager'

const skillQueryKeys = {
  all: ['skills'] as const,
  catalogDetail: (sourceId: string, entryId: string | null, version: string | null) =>
    [...skillQueryKeys.all, 'catalog', 'detail', sourceId, entryId, version] as const,
  catalogEntries: (sourceId: string, query: string, cursor: string | null) =>
    [...skillQueryKeys.all, 'catalog', 'entries', sourceId, query, cursor] as const,
  catalogFile: (
    sourceId: string,
    entryId: string | null,
    version: string | null,
    path: string | null,
  ) => [...skillQueryKeys.all, 'catalog', 'file', sourceId, entryId, version, path] as const,
  catalogInstallTasks: () => [...skillQueryKeys.all, 'catalog', 'installTasks'] as const,
  catalogSources: () => [...skillQueryKeys.all, 'catalog', 'sources'] as const,
  detail: (id: string | null) => [...skillQueryKeys.all, 'detail', id] as const,
  file: (id: string | null, path: string | null) =>
    [...skillQueryKeys.all, 'file', id, path] as const,
  list: () => [...skillQueryKeys.all, 'list'] as const,
}

const SKILLS_PAGE_SIZE = 8
const CATALOG_PAGE_SIZE = 12

type SkillStatusFilter = 'all' | SkillSummary['status']
type SkillSourceFilter = 'all' | SkillSummary['sourceKind']
type SkillSettingsTab = 'skills' | 'tools' | 'mcp' | 'plugins'
type SkillCatalogSourceId = SkillCatalogSource['id']
type CatalogInstallMutationRequest = {
  entry: SkillCatalogEntry
  operationId: string
}

function useSkills() {
  const commandClient = useCommandClient()

  return useQuery({
    queryKey: skillQueryKeys.list(),
    queryFn: () => listSkills(commandClient),
  })
}

function useSkillDetail(id: string | null) {
  const commandClient = useCommandClient()

  return useQuery({
    enabled: id !== null,
    queryKey: skillQueryKeys.detail(id),
    queryFn: () => getSkillDetail(id ?? '', commandClient),
  })
}

function useSkillFile(id: string | null, path: string | null) {
  const commandClient = useCommandClient()

  return useQuery({
    enabled: id !== null && path !== null,
    queryKey: skillQueryKeys.file(id, path),
    queryFn: () => getSkillFile(id ?? '', path ?? '', commandClient),
  })
}

function useImportSkill() {
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: (sourcePath: string) => importSkill(sourcePath, commandClient),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: skillQueryKeys.all })
    },
  })
}

function useSetSkillEnabled() {
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: ({ enabled, id }: { enabled: boolean; id: string }) =>
      setSkillEnabled(id, enabled, commandClient),
    onSuccess: async (response) => {
      await queryClient.invalidateQueries({ queryKey: skillQueryKeys.list() })
      await queryClient.invalidateQueries({
        queryKey: [...skillQueryKeys.all, 'detail', response.skill.id],
      })
    },
  })
}

function useDeleteSkill() {
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: (id: string) => deleteSkill(id, commandClient),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: skillQueryKeys.all })
    },
  })
}

function useSkillCatalogSources() {
  const commandClient = useCommandClient()

  return useQuery({
    queryKey: skillQueryKeys.catalogSources(),
    queryFn: () => listSkillCatalogSources(commandClient),
  })
}

function useSkillCatalogEntries(
  sourceId: SkillCatalogSourceId,
  query: string,
  cursor: string | null,
) {
  const commandClient = useCommandClient()

  return useQuery({
    queryKey: skillQueryKeys.catalogEntries(sourceId, query, cursor),
    queryFn: () =>
      listSkillCatalogEntries(
        {
          cursor: cursor ?? undefined,
          limit: CATALOG_PAGE_SIZE,
          query: query.trim() || undefined,
          sourceId,
        },
        commandClient,
      ),
  })
}

function useSkillCatalogEntry(
  sourceId: SkillCatalogSourceId,
  entry: SkillCatalogEntry | null,
  enabled: boolean,
) {
  const commandClient = useCommandClient()

  return useQuery({
    enabled: enabled && entry !== null,
    queryKey: skillQueryKeys.catalogDetail(
      sourceId,
      entry?.entryId ?? null,
      entry?.version ?? null,
    ),
    queryFn: () =>
      getSkillCatalogEntry(
        {
          entryId: entry?.entryId ?? '',
          sourceId,
          version: entry?.version,
        },
        commandClient,
      ),
  })
}

function useSkillCatalogFile(
  sourceId: SkillCatalogSourceId,
  entry: SkillCatalogEntry | null,
  path: string | null,
  enabled: boolean,
) {
  const commandClient = useCommandClient()

  return useQuery({
    enabled: enabled && entry !== null && path !== null,
    queryKey: skillQueryKeys.catalogFile(
      sourceId,
      entry?.entryId ?? null,
      entry?.version ?? null,
      path,
    ),
    queryFn: () =>
      getSkillCatalogFile(
        {
          entryId: entry?.entryId ?? '',
          path: path ?? '',
          sourceId,
          version: entry?.version,
        },
        commandClient,
      ),
  })
}

function useSkillCatalogInstallTasks() {
  const commandClient = useCommandClient()

  return useQuery({
    queryKey: skillQueryKeys.catalogInstallTasks(),
    queryFn: () => listSkillCatalogInstallTasks(commandClient),
    refetchInterval: (query) =>
      query.state.data?.tasks.some((task) => task.status === 'running') ? 1000 : false,
  })
}

function useInstallSkillFromCatalog() {
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: ({ entry, operationId }: CatalogInstallMutationRequest) =>
      installSkillFromCatalog(
        {
          entryId: entry.entryId,
          operationId,
          sourceId: entry.sourceId,
          version: entry.version,
        },
        commandClient,
      ),
    onSuccess: async (response) => {
      queryClient.setQueryData(
        skillQueryKeys.catalogInstallTasks(),
        upsertCatalogInstallTask(response.task),
      )
      await queryClient.invalidateQueries({ queryKey: skillQueryKeys.list() })
    },
  })
}

function catalogInstallTaskKey(input: {
  entryId: string
  sourceId: string
  version?: string | null
}) {
  return `${input.sourceId}\u0000${input.entryId}\u0000${input.version ?? ''}`
}

function findCatalogInstallTask(
  tasks: SkillCatalogInstallTask[],
  entry: SkillCatalogEntry | null | undefined,
) {
  if (!entry) {
    return null
  }

  const entryKey = catalogInstallTaskKey(entry)
  return tasks.find((task) => catalogInstallTaskKey(task) === entryKey) ?? null
}

function upsertCatalogInstallTask(nextTask: SkillCatalogInstallTask) {
  return (current?: { tasks: SkillCatalogInstallTask[] }) => {
    const currentTasks = current?.tasks ?? []
    const nextKey = catalogInstallTaskKey(nextTask)
    const existing = currentTasks.find((task) => catalogInstallTaskKey(task) === nextKey)
    if (existing && existing.updatedAt > nextTask.updatedAt) {
      return { tasks: currentTasks }
    }

    const tasks = currentTasks.filter((task) => catalogInstallTaskKey(task) !== nextKey)
    return { tasks: [...tasks, nextTask] }
  }
}

function catalogInstallTaskFromProgress(
  progress: SkillCatalogInstallProgressPayload,
  existing: SkillCatalogInstallTask | null,
): SkillCatalogInstallTask {
  const now = new Date().toISOString()
  const status =
    progress.stage === 'completed'
      ? 'completed'
      : progress.stage === 'failed'
        ? 'failed'
        : 'running'

  return {
    entryId: progress.entryId,
    message: progress.message,
    operationId: progress.operationId,
    percent: progress.percent,
    sourceId: progress.sourceId,
    stage: progress.stage,
    startedAt: existing?.startedAt ?? now,
    status,
    updatedAt: now,
    version: progress.version,
  }
}

type CatalogFileSummary = {
  kind: SkillFile['kind']
  path: string
  sizeBytes?: number
}

function buildCatalogFileTree(files: CatalogFileSummary[]): SkillFile[] {
  return files.map((file) => {
    const parts = file.path.split('/').filter(Boolean)

    return {
      depth: Math.max(0, parts.length - 1),
      kind: file.kind,
      name: parts.at(-1) ?? file.path,
      path: file.path,
      sizeBytes: file.sizeBytes,
    }
  })
}

function commandErrorMessage(error: unknown, fallback: string): string {
  if (typeof error === 'object' && error !== null && 'message' in error) {
    const message = (error as { message?: unknown }).message
    if (typeof message === 'string' && message.trim().length > 0) {
      return message
    }
  }

  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message
  }

  return fallback
}

function catalogFileErrorMessage(error: unknown, fallback: string, previewUnavailable: string) {
  const message = commandErrorMessage(error, fallback)
  return message === 'catalog text file must be valid UTF-8' ? previewUnavailable : message
}

function createCatalogInstallOperationId() {
  const randomUUID = globalThis.crypto?.randomUUID?.()
  if (randomUUID) {
    return `catalog-install-${randomUUID}`
  }

  return `catalog-install-${Date.now()}-${Math.random().toString(36).slice(2)}`
}

export function SkillSettingsPage() {
  const { t } = useTranslation('skills')
  const navigate = useNavigate()
  const requestedTab = useRouterState({
    select: (state) => state.location.search.tab,
  })
  const [activeTab, setActiveTab] = useState<SkillSettingsTab>(
    isSkillSettingsTab(requestedTab) ? requestedTab : 'skills',
  )
  const [openPluginRequest, setOpenPluginRequest] = useState<PluginOpenRequest | null>(null)

  useEffect(() => {
    if (isSkillSettingsTab(requestedTab) && requestedTab !== activeTab) {
      setActiveTab(requestedTab)
    }
  }, [activeTab, requestedTab])

  function openPlugin(pluginId: string) {
    setOpenPluginRequest((current) => ({
      pluginId,
      requestId: (current?.requestId ?? 0) + 1,
    }))
    selectTab('plugins')
  }

  function selectTab(tab: SkillSettingsTab) {
    setActiveTab(tab)
    void navigate({ search: { tab }, to: '/skills' })
  }

  return (
    <section aria-label={t('pageTitle')} className="h-full min-h-0 overflow-y-auto pr-1">
      <div className="mx-auto flex w-full max-w-5xl flex-col gap-3 pb-6">
        <Tabs
          className="min-h-0"
          onValueChange={(value) => {
            if (isSkillSettingsTab(value)) {
              selectTab(value)
            }
          }}
          value={activeTab}
        >
          <TabsList aria-label={t('tabs.label')}>
            <TabsTrigger value="skills">{t('tabs.skills')}</TabsTrigger>
            <TabsTrigger value="tools">{t('tabs.tools')}</TabsTrigger>
            <TabsTrigger value="mcp">{t('tabs.mcp')}</TabsTrigger>
            <TabsTrigger value="plugins">{t('tabs.plugins')}</TabsTrigger>
          </TabsList>

          <TabsContent className="space-y-5 pt-3" value="skills">
            <SkillsManager onOpenPlugin={openPlugin} />
          </TabsContent>
          <TabsContent className="space-y-5 pt-3" value="tools">
            <RuntimeToolsList />
          </TabsContent>
          <TabsContent className="space-y-5 pt-3" value="mcp">
            <MCPManager onOpenPlugin={openPlugin} />
          </TabsContent>
          <TabsContent className="space-y-5 pt-3" value="plugins">
            <PluginsManager openPluginRequest={openPluginRequest} />
          </TabsContent>
        </Tabs>
      </div>
    </section>
  )
}

function isSkillSettingsTab(value: unknown): value is SkillSettingsTab {
  return value === 'skills' || value === 'tools' || value === 'mcp' || value === 'plugins'
}

export function SkillsManager({
  onOpenPlugin,
}: {
  onOpenPlugin?: (pluginId: string) => void
} = {}) {
  const { t } = useTranslation('skills')

  return (
    <Tabs className="min-h-0" defaultValue="installed">
      <TabsList aria-label={t('managerTabs.label')}>
        <TabsTrigger value="installed">{t('managerTabs.installed')}</TabsTrigger>
        <TabsTrigger value="catalog">{t('managerTabs.catalog')}</TabsTrigger>
      </TabsList>
      <TabsContent className="space-y-5 pt-3" value="installed">
        <InstalledSkillsManager onOpenPlugin={onOpenPlugin} />
      </TabsContent>
      <TabsContent className="space-y-5 pt-3" value="catalog">
        <SkillCatalogManager />
      </TabsContent>
    </Tabs>
  )
}

function InstalledSkillsManager({ onOpenPlugin }: { onOpenPlugin?: (pluginId: string) => void }) {
  const { t } = useTranslation('skills')
  const skillsQuery = useSkills()
  const importMutation = useImportSkill()
  const setEnabledMutation = useSetSkillEnabled()
  const deleteMutation = useDeleteSkill()
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [selectedFilePath, setSelectedFilePath] = useState<string | null>(null)
  const [search, setSearch] = useState('')
  const [statusFilter, setStatusFilter] = useState<SkillStatusFilter>('all')
  const [sourceFilter, setSourceFilter] = useState<SkillSourceFilter>('all')
  const [pendingDeleteSkill, setPendingDeleteSkill] = useState<SkillSummary | null>(null)
  const [page, setPage] = useState(1)
  const skills = skillsQuery.data?.skills ?? []
  const selectedSkill = skills.find((skill) => skill.id === selectedId) ?? null
  const detailQuery = useSkillDetail(selectedId)
  const fileQuery = useSkillFile(selectedId, selectedFilePath)
  const filteredSkills = useMemo(() => {
    const normalizedSearch = search.trim().toLowerCase()

    return skills.filter((skill) => {
      const matchesSearch =
        normalizedSearch.length === 0 ||
        skill.name.toLowerCase().includes(normalizedSearch) ||
        skill.description.toLowerCase().includes(normalizedSearch)
      const matchesStatus = statusFilter === 'all' || skill.status === statusFilter
      const matchesSource = sourceFilter === 'all' || skill.sourceKind === sourceFilter

      return matchesSearch && matchesStatus && matchesSource
    })
  }, [search, skills, sourceFilter, statusFilter])
  const pageCount = Math.max(1, Math.ceil(filteredSkills.length / SKILLS_PAGE_SIZE))
  const paginatedSkills = filteredSkills.slice(
    (page - 1) * SKILLS_PAGE_SIZE,
    page * SKILLS_PAGE_SIZE,
  )

  useEffect(() => {
    if (page > pageCount) {
      setPage(pageCount)
    }
  }, [page, pageCount])

  async function pickAndImportSkill() {
    const selected = await pickSkillPackagePath()

    if (selected === null) {
      return
    }

    await importMutation.mutateAsync(selected)
  }

  function selectSkill(id: string) {
    setSelectedId(id)
    setSelectedFilePath(null)
  }

  function updateSearch(value: string) {
    setSearch(value)
    setPage(1)
  }

  function updateStatusFilter(value: SkillStatusFilter) {
    setStatusFilter(value)
    setPage(1)
  }

  function updateSourceFilter(value: SkillSourceFilter) {
    setSourceFilter(value)
    setPage(1)
  }

  async function toggleSkill(skill: SkillSummary) {
    if (!skill.manageable) {
      return
    }

    await setEnabledMutation.mutateAsync({
      enabled: !skill.enabled,
      id: skill.id,
    })
  }

  async function removeSkill(skill: SkillSummary) {
    if (!skill.manageable) {
      return
    }

    await deleteMutation.mutateAsync(skill.id)
    setPendingDeleteSkill(null)
    if (selectedId === skill.id) {
      setSelectedId(null)
      setSelectedFilePath(null)
    }
  }

  function requestDeleteSkill(skill: SkillSummary) {
    if (!skill.manageable) {
      return
    }

    setPendingDeleteSkill(skill)
  }

  return (
    <section className="space-y-5">
      <div className="grid gap-3 md:grid-cols-[minmax(0,1fr)_160px_160px_auto]">
        <label className="relative block text-sm" htmlFor="skill-settings-search">
          <span className="sr-only">{t('filters.search')}</span>
          <Search className="absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            className="h-10 pl-9"
            id="skill-settings-search"
            onChange={(event) => updateSearch(event.target.value)}
            placeholder={t('filters.search')}
            value={search}
          />
        </label>
        <label className="block text-sm" htmlFor="skill-settings-status-filter">
          <span className="sr-only">{t('filters.status')}</span>
          <Select
            className="h-10"
            id="skill-settings-status-filter"
            onChange={(event) => updateStatusFilter(event.target.value as SkillStatusFilter)}
            value={statusFilter}
          >
            <option value="all">{t('filters.allStatuses')}</option>
            <option value="ready">{t('status.ready')}</option>
            <option value="disabled">{t('status.disabled')}</option>
            <option value="prerequisite_missing">{t('status.prerequisite_missing')}</option>
            <option value="rejected">{t('status.rejected')}</option>
          </Select>
        </label>
        <label className="block text-sm" htmlFor="skill-settings-source-filter">
          <span className="sr-only">{t('filters.source')}</span>
          <Select
            className="h-10"
            id="skill-settings-source-filter"
            onChange={(event) => updateSourceFilter(event.target.value as SkillSourceFilter)}
            value={sourceFilter}
          >
            <option value="all">{t('filters.allSources')}</option>
            <option value="workspace">{t('source.workspace')}</option>
            <option value="user">{t('source.user')}</option>
            <option value="bundled">{t('source.bundled')}</option>
            <option value="plugin">{t('source.plugin')}</option>
            <option value="mcp">{t('source.mcp')}</option>
          </Select>
        </label>
        <Button
          className="h-10"
          disabled={importMutation.isPending}
          onClick={pickAndImportSkill}
          type="button"
        >
          <Upload data-icon className="size-4" />
          {t('actions.import')}
        </Button>
      </div>

      {skillsQuery.isLoading ? (
        <div className="text-muted-foreground text-sm">{t('loading')}</div>
      ) : null}

      {skillsQuery.isError ? (
        <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
          {t('loadError')}
        </div>
      ) : null}

      {importMutation.isError ? (
        <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
          {t('importError')}
        </div>
      ) : null}

      {setEnabledMutation.isError || deleteMutation.isError ? (
        <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
          {t('mutationError')}
        </div>
      ) : null}

      {!skillsQuery.isLoading && !skillsQuery.isError && skills.length === 0 ? (
        <div className="rounded-md border border-dashed border-border bg-background px-4 py-6 text-center text-muted-foreground text-sm">
          {t('empty')}
        </div>
      ) : null}

      {skills.length > 0 ? (
        <div className="grid min-h-[560px] gap-4 lg:grid-cols-[minmax(240px,300px)_minmax(0,1fr)]">
          <section
            aria-label={t('listLabel')}
            className="flex min-h-0 flex-col rounded-md border border-border bg-background"
          >
            <div className="border-border border-b px-3 py-2 text-muted-foreground text-sm">
              {t('pagination.total', { count: filteredSkills.length })}
            </div>
            <nav className="min-h-0 flex-1 space-y-2 overflow-y-auto p-2">
              {filteredSkills.length === 0 ? (
                <div className="rounded-md border border-dashed border-border px-4 py-6 text-center text-muted-foreground text-sm">
                  {t('noResults')}
                </div>
              ) : null}
              {paginatedSkills.map((skill) => (
                <SkillListItem
                  deletePending={deleteMutation.isPending}
                  key={skill.id}
                  onDelete={requestDeleteSkill}
                  onOpenPlugin={onOpenPlugin}
                  onSelect={selectSkill}
                  onToggle={toggleSkill}
                  selected={selectedId === skill.id}
                  skill={skill}
                  togglePending={setEnabledMutation.isPending}
                />
              ))}
            </nav>
            <div className="flex items-center justify-between gap-2 border-border border-t p-2">
              <Button
                aria-label={t('pagination.previous')}
                disabled={page <= 1}
                onClick={() => setPage((current) => Math.max(1, current - 1))}
                size="sm"
                type="button"
                variant="outline"
              >
                <ChevronLeft data-icon className="size-4" />
              </Button>
              <span className="text-muted-foreground text-sm">
                {t('pagination.pageInfo', { page, pageCount })}
              </span>
              <Button
                aria-label={t('pagination.next')}
                disabled={page >= pageCount}
                onClick={() => setPage((current) => Math.min(pageCount, current + 1))}
                size="sm"
                type="button"
                variant="outline"
              >
                <ChevronRight data-icon className="size-4" />
              </Button>
            </div>
          </section>

          <SkillDetailPanel
            detailQuery={detailQuery}
            fileQuery={fileQuery}
            onSelectFile={setSelectedFilePath}
            selectedFilePath={selectedFilePath}
            selectedSkill={selectedSkill}
          />
        </div>
      ) : null}

      <Dialog
        onOpenChange={(open) => {
          if (!open) {
            setPendingDeleteSkill(null)
          }
        }}
        open={pendingDeleteSkill !== null}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('actions.confirmDeleteTitle')}</DialogTitle>
            <DialogDescription>
              {t('actions.confirmDeleteDescription', { name: pendingDeleteSkill?.name ?? '' })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              disabled={deleteMutation.isPending}
              onClick={() => setPendingDeleteSkill(null)}
              type="button"
              variant="outline"
            >
              {t('actions.cancel')}
            </Button>
            <Button
              disabled={deleteMutation.isPending || pendingDeleteSkill === null}
              onClick={() => {
                if (pendingDeleteSkill) {
                  void removeSkill(pendingDeleteSkill)
                }
              }}
              type="button"
              variant="destructive"
            >
              <Trash2 data-icon className="size-4" />
              {t('actions.confirmDelete')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </section>
  )
}

function SkillCatalogManager() {
  const { t } = useTranslation('skills')
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const sourcesQuery = useSkillCatalogSources()
  const catalogInstallTasksQuery = useSkillCatalogInstallTasks()
  const installMutation = useInstallSkillFromCatalog()
  const [sourceId, setSourceId] = useState<SkillCatalogSourceId>('anthropic')
  const [search, setSearch] = useState('')
  const [cursor, setCursor] = useState<string | null>(null)
  const [cursorHistory, setCursorHistory] = useState<(string | null)[]>([])
  const [selectedEntry, setSelectedEntry] = useState<SkillCatalogEntry | null>(null)
  const [selectedCatalogFilePath, setSelectedCatalogFilePath] = useState<string | null>(null)
  const [catalogInstallSetupError, setCatalogInstallSetupError] = useState<unknown>(null)
  const [detailOpen, setDetailOpen] = useState(false)
  const entriesQuery = useSkillCatalogEntries(sourceId, search, cursor)
  const entries = entriesQuery.data?.entries ?? []
  const catalogInstallTasks = catalogInstallTasksQuery.data?.tasks ?? []
  const detailQuery = useSkillCatalogEntry(sourceId, selectedEntry, detailOpen)
  const catalogFileQuery = useSkillCatalogFile(
    sourceId,
    selectedEntry,
    selectedCatalogFilePath,
    detailOpen,
  )
  const selectedSource = sourcesQuery.data?.sources.find((source) => source.id === sourceId)
  const detailEntry = detailQuery.data?.entry ?? selectedEntry
  const catalogFiles = useMemo(
    () => buildCatalogFileTree(detailQuery.data?.files ?? []),
    [detailQuery.data?.files],
  )
  const selectedCatalogInstallTask = findCatalogInstallTask(catalogInstallTasks, selectedEntry)
  const selectedCatalogInstallInFlight = selectedCatalogInstallTask?.status === 'running'
  const selectedCatalogInstallCompleted = selectedCatalogInstallTask?.status === 'completed'
  const catalogInstallDisabled =
    installMutation.isPending ||
    selectedCatalogInstallInFlight ||
    selectedCatalogInstallCompleted ||
    detailQuery.isLoading ||
    !detailQuery.data ||
    detailEntry?.installable === false ||
    detailEntry?.installed === true ||
    detailQuery.data.validation.status === 'blocked'
  const catalogInstallPercent = selectedCatalogInstallTask?.percent ?? 5
  const catalogInstallStage = selectedCatalogInstallTask?.stage ?? 'preparing'

  useEffect(() => {
    if (selectedCatalogFilePath !== null || catalogFiles.length === 0) {
      return
    }
    const skillFile =
      catalogFiles.find((file) => file.kind === 'file' && file.path === 'SKILL.md') ??
      catalogFiles.find((file) => file.kind === 'file')
    if (skillFile) {
      setSelectedCatalogFilePath(skillFile.path)
    }
  }, [catalogFiles, selectedCatalogFilePath])

  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | null = null

    listenSkillCatalogInstallProgress((progress) => {
      queryClient.setQueryData(skillQueryKeys.catalogInstallTasks(), (current) => {
        const tasks = (current as { tasks: SkillCatalogInstallTask[] } | undefined)?.tasks ?? []
        const existing =
          tasks.find((task) => task.operationId === progress.operationId) ??
          tasks.find((task) => catalogInstallTaskKey(task) === catalogInstallTaskKey(progress)) ??
          null
        const nextTask = catalogInstallTaskFromProgress(progress, existing)
        return upsertCatalogInstallTask(nextTask)({ tasks })
      })

      if (progress.stage === 'completed' || progress.stage === 'failed') {
        void queryClient.invalidateQueries({ queryKey: skillQueryKeys.list() })
        void queryClient.invalidateQueries({ queryKey: [...skillQueryKeys.all, 'catalog'] })
      }
    }, commandClient)
      .then((cleanup) => {
        if (disposed) {
          cleanup()
          return
        }
        unlisten = cleanup
      })
      .catch((error: unknown) => {
        setCatalogInstallSetupError(error)
      })

    return () => {
      disposed = true
      unlisten?.()
    }
  }, [commandClient, queryClient])

  function resetCatalogSelection() {
    setSelectedEntry(null)
    setSelectedCatalogFilePath(null)
    setCatalogInstallSetupError(null)
    setDetailOpen(false)
    setCursor(null)
    setCursorHistory([])
    installMutation.reset()
  }

  function selectSource(nextSourceId: SkillCatalogSourceId) {
    setSourceId(nextSourceId)
    setSearch('')
    resetCatalogSelection()
  }

  function updateCatalogSearch(value: string) {
    setSearch(value)
    resetCatalogSelection()
  }

  function openEntry(entry: SkillCatalogEntry) {
    setSelectedEntry(entry)
    setSelectedCatalogFilePath(null)
    setCatalogInstallSetupError(null)
    installMutation.reset()
    setDetailOpen(true)
  }

  function goToNextPage() {
    const nextCursor = entriesQuery.data?.nextCursor
    if (!nextCursor) {
      return
    }
    setCursorHistory((current) => [...current, cursor])
    setCursor(nextCursor)
  }

  function goToPreviousPage() {
    setCursorHistory((current) => {
      const nextHistory = current.slice(0, -1)
      setCursor(current.at(-1) ?? null)
      return nextHistory
    })
  }

  function catalogSourceLabel(source: SkillCatalogSource) {
    return t(`catalog.sourcesById.${source.id}.label`, { defaultValue: source.label })
  }

  function catalogSourceDescription(source: SkillCatalogSource) {
    return t(`catalog.sourcesById.${source.id}.description`, {
      defaultValue: source.description,
    })
  }

  function catalogSourceLabelById(entry: SkillCatalogEntry) {
    const source = sourcesQuery.data?.sources.find((candidate) => candidate.id === entry.sourceId)
    return source ? catalogSourceLabel(source) : entry.sourceLabel
  }

  function installSelectedEntry() {
    if (selectedEntry === null || catalogInstallDisabled) {
      return
    }

    const operationId = createCatalogInstallOperationId()
    const startedAt = new Date().toISOString()
    queryClient.setQueryData(
      skillQueryKeys.catalogInstallTasks(),
      upsertCatalogInstallTask({
        entryId: selectedEntry.entryId,
        operationId,
        percent: 5,
        sourceId: selectedEntry.sourceId,
        stage: 'preparing',
        startedAt,
        status: 'running',
        updatedAt: startedAt,
        version: selectedEntry.version,
      }),
    )
    setCatalogInstallSetupError(null)

    installMutation.mutate(
      { entry: selectedEntry, operationId },
      {
        onError: (error) => {
          const failedAt = new Date().toISOString()
          queryClient.setQueryData(
            skillQueryKeys.catalogInstallTasks(),
            upsertCatalogInstallTask({
              entryId: selectedEntry.entryId,
              message: commandErrorMessage(error, t('catalog.installError')),
              operationId,
              percent: 100,
              sourceId: selectedEntry.sourceId,
              stage: 'failed',
              startedAt,
              status: 'failed',
              updatedAt: failedAt,
              version: selectedEntry.version,
            }),
          )
        },
      },
    )
  }

  return (
    <section className="grid min-h-[560px] gap-4 lg:grid-cols-[240px_minmax(0,1fr)]">
      <section
        aria-label={t('catalog.sourcesLabel')}
        className="rounded-md border border-border bg-background"
      >
        <div className="border-border border-b px-3 py-2 font-medium text-sm">
          {t('catalog.sources')}
        </div>
        <div className="space-y-2 p-2">
          {sourcesQuery.isLoading ? (
            <div className="px-2 py-3 text-muted-foreground text-sm">{t('catalog.loading')}</div>
          ) : null}
          {sourcesQuery.isError ? (
            <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
              {t('catalog.sourceError')}
            </div>
          ) : null}
          {sourcesQuery.data?.sources.map((source) => (
            <button
              className="block w-full rounded-md border border-border bg-surface px-3 py-2 text-left text-sm transition-colors data-[selected=true]:border-primary data-[selected=true]:bg-muted/35"
              data-selected={source.id === sourceId}
              key={source.id}
              onClick={() => selectSource(source.id)}
              type="button"
            >
              <span className="flex items-center justify-between gap-2">
                <span className="font-medium">{catalogSourceLabel(source)}</span>
                <Badge variant="outline">{t(`catalog.trust.${source.trustLevel}`)}</Badge>
              </span>
              <span className="mt-1 block text-muted-foreground text-xs">
                {catalogSourceDescription(source)}
              </span>
            </button>
          ))}
        </div>
      </section>

      <section
        aria-label={t('catalog.entriesLabel')}
        className="flex min-h-0 flex-col rounded-md border border-border bg-background"
      >
        <div className="border-border border-b p-2">
          <label className="relative block text-sm" htmlFor="skill-catalog-search">
            <span className="sr-only">{t('catalog.search')}</span>
            <Search className="absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              className="h-10 pl-9"
              id="skill-catalog-search"
              onChange={(event) => updateCatalogSearch(event.target.value)}
              placeholder={t('catalog.search')}
              value={search}
            />
          </label>
        </div>
        <div className="min-h-0 flex-1 space-y-2 overflow-y-auto p-2">
          {entriesQuery.isLoading ? (
            <div className="px-2 py-3 text-muted-foreground text-sm">{t('catalog.loading')}</div>
          ) : null}
          {entriesQuery.isError ? (
            <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
              {t('catalog.entriesError')}
            </div>
          ) : null}
          {!entriesQuery.isLoading && !entriesQuery.isError && entries.length === 0 ? (
            <div className="rounded-md border border-dashed border-border px-4 py-6 text-center text-muted-foreground text-sm">
              {t('catalog.empty')}
            </div>
          ) : null}
          <TooltipProvider delayDuration={250}>
            {entries.map((entry) => {
              const installTask = findCatalogInstallTask(catalogInstallTasks, entry)
              const running = installTask?.status === 'running'

              return (
                <Tooltip key={entry.entryId}>
                  <TooltipTrigger asChild>
                    <button
                      aria-label={entry.name}
                      className="relative block w-full overflow-hidden rounded-md border border-border bg-surface px-3 py-2 text-left text-sm transition-colors hover:border-primary/60 hover:bg-muted/25 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                      onClick={() => openEntry(entry)}
                      type="button"
                    >
                      {running ? (
                        <span
                          aria-hidden="true"
                          className="absolute inset-y-0 left-0 bg-primary/15 transition-[width]"
                          style={{ width: `${installTask.percent}%` }}
                        />
                      ) : null}
                      <span className="relative z-10 block">
                        <span className="flex items-center justify-between gap-2">
                          <span className="min-w-0 truncate font-medium">{entry.name}</span>
                          {running ? (
                            <span className="shrink-0 font-medium text-primary text-xs">
                              {installTask.percent}%
                            </span>
                          ) : null}
                          {!running && entry.installed ? (
                            <Badge variant="success">{t('catalog.installed')}</Badge>
                          ) : null}
                        </span>
                        <span className="mt-1 line-clamp-2 text-muted-foreground text-xs">
                          {entry.description}
                        </span>
                        <span className="mt-2 flex flex-wrap gap-1">
                          <Badge variant="outline">{catalogSourceLabelById(entry)}</Badge>
                          {entry.version ? <Badge variant="outline">{entry.version}</Badge> : null}
                        </span>
                      </span>
                    </button>
                  </TooltipTrigger>
                  <TooltipContent className="max-w-80 space-y-1.5 leading-5">
                    <div className="font-medium">{entry.name}</div>
                    <div className="text-muted-foreground">{entry.description}</div>
                    <div className="flex flex-wrap gap-1 pt-1">
                      <Badge variant="outline">{catalogSourceLabelById(entry)}</Badge>
                      <Badge variant="outline">{t(`catalog.trust.${entry.trustLevel}`)}</Badge>
                      {entry.tags.slice(0, 3).map((tag) => (
                        <Badge key={tag} variant="outline">
                          {tag}
                        </Badge>
                      ))}
                    </div>
                  </TooltipContent>
                </Tooltip>
              )
            })}
          </TooltipProvider>
        </div>
        <div className="flex items-center justify-between gap-2 border-border border-t p-2">
          <Button
            disabled={cursorHistory.length === 0 || entriesQuery.isFetching}
            onClick={goToPreviousPage}
            size="sm"
            type="button"
            variant="outline"
          >
            <ChevronLeft data-icon className="size-4" />
            {t('pagination.previous')}
          </Button>
          <span className="text-muted-foreground text-sm">
            {t('catalog.pageInfo', { page: cursorHistory.length + 1 })}
          </span>
          <Button
            disabled={!entriesQuery.data?.nextCursor || entriesQuery.isFetching}
            onClick={goToNextPage}
            size="sm"
            type="button"
            variant="outline"
          >
            {t('pagination.next')}
            <ChevronRight data-icon className="size-4" />
          </Button>
        </div>
      </section>

      <Dialog
        onOpenChange={(open) => {
          setDetailOpen(open)
          if (!open) {
            setSelectedEntry(null)
          }
        }}
        open={detailOpen}
      >
        <DialogContent className="max-h-[min(780px,calc(100vh-2rem))] w-[min(calc(100vw-2rem),68rem)] overflow-hidden p-0">
          {selectedEntry === null ? null : (
            <div className="grid min-h-[620px] min-w-0 grid-cols-[minmax(200px,260px)_minmax(0,1fr)]">
              <section
                aria-label={t('catalog.files')}
                className="min-h-0 border-border border-r bg-background"
              >
                <div className="border-border border-b px-3 py-2 font-medium text-sm">
                  {t('catalog.files')}
                </div>
                {detailQuery.isLoading ? (
                  <div className="p-3 text-muted-foreground text-sm">
                    {t('catalog.loadingDetail')}
                  </div>
                ) : null}
                {!detailQuery.isLoading && catalogFiles.length === 0 ? (
                  <div className="p-3 text-muted-foreground text-sm">{t('catalog.noFiles')}</div>
                ) : null}
                {catalogFiles.length > 0 ? (
                  <div className="max-h-[576px] overflow-y-auto p-2">
                    {catalogFiles.map((file) => (
                      <SkillFileRow
                        file={file}
                        key={file.path}
                        onSelectFile={setSelectedCatalogFilePath}
                        selected={file.path === selectedCatalogFilePath}
                      />
                    ))}
                  </div>
                ) : null}
              </section>

              <section className="flex min-h-0 min-w-0 flex-col">
                <div className="border-border border-b p-5">
                  <div className="flex items-start justify-between gap-4">
                    <DialogHeader className="min-w-0">
                      <DialogTitle>{detailEntry?.name ?? selectedEntry.name}</DialogTitle>
                      <DialogDescription>
                        {detailEntry?.description ?? selectedEntry.description}
                      </DialogDescription>
                    </DialogHeader>
                    {selectedEntry.installable ? (
                      <Button
                        className="relative min-w-36 overflow-hidden"
                        disabled={catalogInstallDisabled}
                        onClick={installSelectedEntry}
                        type="button"
                      >
                        {selectedCatalogInstallInFlight ? (
                          <>
                            <span
                              aria-hidden="true"
                              className="absolute inset-y-0 left-0 bg-primary/20 transition-[width]"
                              style={{ width: `${catalogInstallPercent}%` }}
                            />
                            <span className="relative z-10 inline-flex items-center gap-2">
                              <Download data-icon className="size-4" />
                              {t(`catalog.installProgress.${catalogInstallStage}`)}{' '}
                              {catalogInstallPercent}%
                            </span>
                          </>
                        ) : (
                          <>
                            <Download data-icon className="size-4" />
                            {detailEntry?.installed || selectedCatalogInstallCompleted
                              ? t('catalog.installed')
                              : t('catalog.install')}
                          </>
                        )}
                      </Button>
                    ) : null}
                  </div>

                  <div className="mt-3 flex flex-wrap gap-1">
                    <Badge variant="outline">{catalogSourceLabelById(selectedEntry)}</Badge>
                    <Badge variant="outline">
                      {t(`catalog.trust.${selectedEntry.trustLevel}`)}
                    </Badge>
                    {detailQuery.data?.validation.status ? (
                      <Badge variant="outline">
                        {t(`catalog.validation.${detailQuery.data.validation.status}`)}
                      </Badge>
                    ) : null}
                    {selectedEntry.tags.map((tag) => (
                      <Badge key={tag} variant="outline">
                        {tag}
                      </Badge>
                    ))}
                  </div>

                  <div className="mt-3 space-y-2">
                    {detailQuery.isLoading ? (
                      <div className="text-muted-foreground text-sm">
                        {t('catalog.loadingDetail')}
                      </div>
                    ) : null}
                    {detailQuery.isError ? (
                      <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
                        {commandErrorMessage(detailQuery.error, t('catalog.detailError'))}
                      </div>
                    ) : null}
                    {catalogInstallSetupError !== null || installMutation.isError ? (
                      <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
                        {commandErrorMessage(
                          catalogInstallSetupError ?? installMutation.error,
                          t('catalog.installError'),
                        )}
                      </div>
                    ) : null}
                    {selectedCatalogInstallTask?.status === 'failed' && !installMutation.isError ? (
                      <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
                        {selectedCatalogInstallTask.message ?? t('catalog.installError')}
                      </div>
                    ) : null}
                    {detailQuery.data?.validation.issues.length ? (
                      <div className="rounded-md border border-border bg-muted/25 px-3 py-2 text-sm">
                        <div className="font-medium">{t('catalog.validationIssues')}</div>
                        <ul className="mt-2 list-disc space-y-1 pl-5 text-muted-foreground">
                          {detailQuery.data.validation.issues.map((issue) => (
                            <li key={issue}>{issue}</li>
                          ))}
                        </ul>
                      </div>
                    ) : null}
                    {selectedSource?.installable === false && !detailQuery.data?.readmePreview ? (
                      <div className="text-muted-foreground text-sm">{t('catalog.specOnly')}</div>
                    ) : null}
                  </div>
                </div>

                <CatalogFilePreview
                  fileQuery={catalogFileQuery}
                  readmePreview={detailQuery.data?.readmePreview}
                  selectedFilePath={selectedCatalogFilePath}
                />
              </section>
            </div>
          )}
        </DialogContent>
      </Dialog>
    </section>
  )
}

function CatalogFilePreview({
  fileQuery,
  readmePreview,
  selectedFilePath,
}: {
  fileQuery: ReturnType<typeof useSkillCatalogFile>
  readmePreview: string | undefined
  selectedFilePath: string | null
}) {
  const { t } = useTranslation('skills')
  const selectedFile = fileQuery.data?.file as GetSkillCatalogFileResponse['file'] | undefined
  const title = selectedFile?.path ?? selectedFilePath ?? t('content.title')

  return (
    <section className="min-h-0 min-w-0 flex-1">
      <div className="border-border border-b px-3 py-2 font-medium text-sm">{title}</div>
      <div className="max-h-[420px] min-h-[360px] overflow-auto">
        {selectedFile?.truncated ? (
          <div className="border-border border-b bg-muted/25 px-3 py-2 text-muted-foreground text-sm">
            {t('catalog.fileTruncated')}
          </div>
        ) : null}
        {fileQuery.isError ? (
          <div className="m-3 rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
            {catalogFileErrorMessage(
              fileQuery.error,
              t('catalog.fileLoadError'),
              t('catalog.filePreviewUnavailable'),
            )}
          </div>
        ) : (
          <pre className="min-h-[360px] max-w-full p-3 text-sm leading-6">
            <code className="block min-w-full w-max whitespace-pre">
              {fileQuery.isLoading
                ? t('catalog.loadingFile')
                : (selectedFile?.content ?? readmePreview ?? t('content.empty'))}
            </code>
          </pre>
        )}
      </div>
    </section>
  )
}

function SkillListItem({
  deletePending,
  onDelete,
  onOpenPlugin,
  onSelect,
  onToggle,
  selected,
  skill,
  togglePending,
}: {
  deletePending: boolean
  onDelete: (skill: SkillSummary) => void
  onOpenPlugin?: (pluginId: string) => void
  onSelect: (id: string) => void
  onToggle: (skill: SkillSummary) => void
  selected: boolean
  skill: SkillSummary
  togglePending: boolean
}) {
  const { t } = useTranslation('skills')
  const sourcePluginId =
    skill.sourceKind === 'plugin' && skill.sourcePluginId ? skill.sourcePluginId : null

  return (
    <div
      className="rounded-md border border-border bg-surface px-2.5 py-2 text-sm transition-colors data-[selected=true]:border-primary data-[selected=true]:bg-muted/35"
      data-skill-card
      data-selected={selected}
    >
      <button
        aria-pressed={selected}
        className="block w-full rounded-sm text-left outline-none focus-visible:ring-2 focus-visible:ring-ring"
        onClick={() => onSelect(skill.id)}
        type="button"
      >
        <span className="flex items-center justify-between gap-3">
          <span className="min-w-0">
            <span className="block truncate font-medium text-foreground">{skill.name}</span>
            <span className="mt-0.5 block truncate text-muted-foreground text-xs">
              {skill.description}
            </span>
          </span>
          <Badge variant={skill.enabled ? 'success' : 'outline'}>
            {t(`status.${skill.status}`)}
          </Badge>
        </span>
        <span className="mt-1.5 flex flex-wrap gap-1">
          <Badge variant="outline">{t(`source.${skill.sourceKind}`)}</Badge>
        </span>
      </button>

      {skill.manageable ? (
        <div className="mt-2 flex items-center justify-between gap-2">
          <div className="flex min-w-0 items-center gap-2 text-muted-foreground text-xs">
            <Switch
              aria-label={
                skill.enabled
                  ? t('actions.disableSkill', { name: skill.name })
                  : t('actions.enableSkill', { name: skill.name })
              }
              checked={skill.enabled}
              disabled={togglePending}
              onCheckedChange={() => onToggle(skill)}
            />
            <span className="truncate">
              {skill.enabled ? t('actions.disable') : t('actions.enable')}
            </span>
          </div>
          <Button
            aria-label={t('actions.deleteSkill', { name: skill.name })}
            disabled={deletePending}
            onClick={() => onDelete(skill)}
            size="sm"
            type="button"
            variant="ghost"
          >
            <Trash2 data-icon className="size-4 text-destructive" />
            {t('actions.delete')}
          </Button>
        </div>
      ) : sourcePluginId && onOpenPlugin ? (
        <div className="mt-2 flex justify-end">
          <Button
            aria-label={t('actions.viewSourcePlugin', { pluginId: sourcePluginId })}
            onClick={() => onOpenPlugin(sourcePluginId)}
            size="sm"
            type="button"
            variant="outline"
          >
            <ExternalLink data-icon className="size-4" />
            {t('actions.sourcePlugin')}
          </Button>
        </div>
      ) : null}
    </div>
  )
}

function SkillFilesTab({
  detailQuery,
  fileQuery,
  onSelectFile,
  selectedFilePath,
  selectedSkill,
}: {
  detailQuery: ReturnType<typeof useSkillDetail>
  fileQuery: ReturnType<typeof useSkillFile>
  onSelectFile: (path: string) => void
  selectedFilePath: string | null
  selectedSkill: SkillSummary | null
}) {
  const { t } = useTranslation('skills')
  const files = detailQuery.data?.skill.files ?? []
  const detail = detailQuery.data?.skill ?? null
  const selectedFile = fileQuery.data?.file

  useEffect(() => {
    if (selectedFilePath !== null || files.length === 0) {
      return
    }
    const firstFile = files.find((file) => file.kind === 'file')
    if (firstFile) {
      onSelectFile(firstFile.path)
    }
  }, [files, onSelectFile, selectedFilePath])

  return (
    <section
      aria-label={t('files.title')}
      className="grid min-h-[420px] min-w-0 gap-3 md:grid-cols-[minmax(220px,280px)_minmax(0,1fr)]"
    >
      <div className="min-h-0 rounded-md border border-border bg-surface">
        <div className="border-border border-b px-3 py-2 font-medium text-sm">
          {t('files.title')}
        </div>
        {!selectedSkill ? (
          <div className="p-3 text-muted-foreground text-sm">{t('files.empty')}</div>
        ) : null}
        {selectedSkill && detailQuery.isLoading ? (
          <div className="p-3 text-muted-foreground text-sm">{t('files.loading')}</div>
        ) : null}
        {selectedSkill && detailQuery.isError ? (
          <div className="m-3 rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
            {t('files.loadError')}
          </div>
        ) : null}
        {selectedSkill && !detailQuery.isLoading && !detailQuery.isError && files.length === 0 ? (
          <div className="p-3 text-muted-foreground text-sm">{t('files.noFiles')}</div>
        ) : null}
        {files.length > 0 ? (
          <div className="max-h-[560px] overflow-y-auto p-2">
            {files.map((file) => (
              <SkillFileRow
                file={file}
                key={file.path}
                onSelectFile={onSelectFile}
                selected={file.path === selectedFilePath}
              />
            ))}
          </div>
        ) : null}
      </div>
      <div className="min-w-0 rounded-md border border-border bg-surface">
        <div className="border-border border-b px-3 py-2 font-medium text-sm">
          {selectedFile?.path ?? t('content.title')}
        </div>
        <pre className="max-h-[560px] min-h-[360px] max-w-full overflow-auto p-3 text-sm leading-6">
          <code className="block min-w-full w-max whitespace-pre">
            {fileQuery.isLoading
              ? t('files.loading')
              : (selectedFile?.content ?? detail?.bodyPreview ?? t('content.empty'))}
          </code>
        </pre>
      </div>
    </section>
  )
}

function SkillFileRow({
  file,
  onSelectFile,
  selected,
}: {
  file: SkillFile
  onSelectFile: (path: string) => void
  selected: boolean
}) {
  const { t } = useTranslation('skills')
  const indent = `${file.depth * 14}px`
  const icon =
    file.kind === 'directory' ? (
      <Folder data-icon className="size-4 text-muted-foreground" />
    ) : (
      <FileText data-icon className="size-4 text-muted-foreground" />
    )

  if (file.kind === 'directory') {
    return (
      <div
        className="flex h-8 items-center gap-2 rounded-sm px-2 text-muted-foreground text-sm"
        style={{ paddingLeft: `calc(0.5rem + ${indent})` }}
      >
        {icon}
        <span className="truncate">{file.name}</span>
      </div>
    )
  }

  return (
    <button
      aria-pressed={selected}
      className="flex h-8 w-full items-center gap-2 rounded-sm px-2 text-left text-sm outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring aria-pressed:bg-muted"
      onClick={() => onSelectFile(file.path)}
      style={{ paddingLeft: `calc(0.5rem + ${indent})` }}
      type="button"
    >
      {icon}
      <span className="min-w-0 flex-1 truncate">{file.name}</span>
      {file.sizeBytes === undefined ? null : (
        <span className="text-muted-foreground text-xs">
          {t('files.size', { size: file.sizeBytes })}
        </span>
      )}
    </button>
  )
}

function SkillDetailPanel({
  detailQuery,
  fileQuery,
  onSelectFile,
  selectedFilePath,
  selectedSkill,
}: {
  detailQuery: ReturnType<typeof useSkillDetail>
  fileQuery: ReturnType<typeof useSkillFile>
  onSelectFile: (path: string) => void
  selectedFilePath: string | null
  selectedSkill: SkillSummary | null
}) {
  const { t } = useTranslation('skills')
  const detail = detailQuery.data?.skill ?? null
  const summary = detail?.summary ?? selectedSkill

  return (
    <section
      aria-label={t('detail.title')}
      className="min-h-[420px] rounded-md border border-border bg-background p-4"
    >
      {!selectedSkill ? (
        <div className="flex h-full min-h-[360px] items-center justify-center text-muted-foreground text-sm">
          {t('detail.empty')}
        </div>
      ) : null}

      {selectedSkill && detailQuery.isLoading ? (
        <div className="text-muted-foreground text-sm">{t('detail.loading')}</div>
      ) : null}

      {selectedSkill && detailQuery.isError ? (
        <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
          {t('detail.loadError')}
        </div>
      ) : null}

      {summary ? (
        <div className="space-y-5">
          <div className="min-w-0">
            <h3 className="break-words font-semibold text-lg">{summary.name}</h3>
            <p className="mt-1 text-muted-foreground text-sm">{summary.description}</p>
            <div className="mt-3 flex flex-wrap gap-1.5">
              <Badge variant={summary.enabled ? 'success' : 'outline'}>
                {t(`status.${summary.status}`)}
              </Badge>
              <Badge variant="outline">{t(`source.${summary.sourceKind}`)}</Badge>
              {summary.manageable ? (
                <Badge variant="secondary">{t('manageable')}</Badge>
              ) : (
                <Badge variant="outline">{t('readOnly')}</Badge>
              )}
              {summary.tags.map((tag) => (
                <Badge key={tag} variant="outline">
                  {tag}
                </Badge>
              ))}
            </div>
          </div>

          <Tabs className="min-h-0" defaultValue="overview" key={summary.id}>
            <TabsList aria-label={t('detail.tabsLabel')} className="flex w-fit flex-wrap">
              <TabsTrigger value="overview">{t('detail.tabs.overview')}</TabsTrigger>
              <TabsTrigger value="files">{t('detail.tabs.files')}</TabsTrigger>
              <TabsTrigger value="parameters">{t('detail.tabs.parameters')}</TabsTrigger>
              <TabsTrigger value="config">{t('detail.tabs.config')}</TabsTrigger>
            </TabsList>

            <TabsContent className="pt-2" value="overview">
              <section className="grid gap-3 text-sm sm:grid-cols-2">
                <div className="rounded-md border border-border bg-surface p-3">
                  <div className="text-muted-foreground">{t('filters.status')}</div>
                  <div className="mt-1 font-medium">{t(`status.${summary.status}`)}</div>
                </div>
                <div className="rounded-md border border-border bg-surface p-3">
                  <div className="text-muted-foreground">{t('filters.source')}</div>
                  <div className="mt-1 font-medium">{t(`source.${summary.sourceKind}`)}</div>
                </div>
                <div className="rounded-md border border-border bg-surface p-3">
                  <div className="text-muted-foreground">{t('detail.importedAt')}</div>
                  <div className="mt-1 font-medium">{summary.importedAt ?? '-'}</div>
                </div>
                <div className="rounded-md border border-border bg-surface p-3">
                  <div className="text-muted-foreground">{t('detail.updatedAt')}</div>
                  <div className="mt-1 font-medium">{summary.updatedAt ?? '-'}</div>
                </div>
              </section>
              {detail?.validationError ? (
                <div className="mt-3 rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
                  {t('detail.validationError')}
                </div>
              ) : null}
            </TabsContent>

            <TabsContent className="pt-2" value="files">
              <SkillFilesTab
                detailQuery={detailQuery}
                fileQuery={fileQuery}
                onSelectFile={onSelectFile}
                selectedFilePath={selectedFilePath}
                selectedSkill={selectedSkill}
              />
            </TabsContent>

            <TabsContent className="pt-2" value="parameters">
              {detailQuery.isLoading ? (
                <div className="text-muted-foreground text-sm">{t('detail.loading')}</div>
              ) : null}
              {detail ? (
                <section className="space-y-2">
                  {detail.parameters.length === 0 ? (
                    <p className="text-muted-foreground text-sm">{t('detail.noParameters')}</p>
                  ) : (
                    <div className="overflow-x-auto rounded-md border border-border">
                      <table className="w-full min-w-[520px] text-left text-sm">
                        <thead className="bg-muted text-muted-foreground">
                          <tr>
                            <th className="px-3 py-2 font-medium">{t('detail.paramName')}</th>
                            <th className="px-3 py-2 font-medium">{t('detail.paramType')}</th>
                            <th className="px-3 py-2 font-medium">{t('detail.paramRequired')}</th>
                            <th className="px-3 py-2 font-medium">
                              {t('detail.paramDescription')}
                            </th>
                          </tr>
                        </thead>
                        <tbody>
                          {detail.parameters.map((parameter) => (
                            <tr className="border-border border-t" key={parameter.name}>
                              <td className="px-3 py-2 font-mono">{parameter.name}</td>
                              <td className="px-3 py-2">{parameter.paramType}</td>
                              <td className="px-3 py-2">
                                {parameter.required ? t('yes') : t('no')}
                              </td>
                              <td className="px-3 py-2 text-muted-foreground">
                                {parameter.description ?? ''}
                              </td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  )}
                </section>
              ) : null}
            </TabsContent>

            <TabsContent className="pt-2" value="config">
              {detailQuery.isLoading ? (
                <div className="text-muted-foreground text-sm">{t('detail.loading')}</div>
              ) : null}
              {detail ? (
                <section className="space-y-2">
                  {detail.configKeys.length === 0 ? (
                    <p className="text-muted-foreground text-sm">{t('detail.noConfigKeys')}</p>
                  ) : (
                    <div className="flex flex-wrap gap-1.5">
                      {detail.configKeys.map((key) => (
                        <Badge className="font-mono" key={key} variant="outline">
                          {key}
                        </Badge>
                      ))}
                    </div>
                  )}
                </section>
              ) : null}
            </TabsContent>
          </Tabs>
        </div>
      ) : null}
    </section>
  )
}

export function RuntimeToolsList() {
  const { t } = useTranslation('skills')
  const commandClient = useCommandClient()
  const [query, setQuery] = useState('')
  const toolsQuery = useQuery({
    queryKey: ['runtime-tools'],
    queryFn: () => listRuntimeTools(commandClient),
  })
  const tools = toolsQuery.data?.tools ?? []
  const normalizedQuery = query.trim().toLowerCase()
  const groupLabelForTool = useCallback(
    (tool: RuntimeToolSummary) =>
      t(`tools.groups.${tool.group}`, { defaultValue: tool.groupLabel }),
    [t],
  )
  const filteredTools = useMemo(() => {
    if (!normalizedQuery) {
      return tools
    }
    return tools.filter((tool) =>
      [
        tool.name,
        tool.displayName,
        tool.description,
        tool.group,
        groupLabelForTool(tool),
        tool.originKind,
        tool.originId ?? '',
        tool.executionChannel,
      ]
        .join(' ')
        .toLowerCase()
        .includes(normalizedQuery),
    )
  }, [groupLabelForTool, normalizedQuery, tools])

  return (
    <section className="rounded-md border border-border bg-surface">
      <div className="flex items-start justify-between gap-4 border-border border-b p-5">
        <div className="flex items-start gap-3">
          <div className="rounded-md border border-border bg-background p-2 text-muted-foreground">
            <Wrench className="size-4" />
          </div>
          <div>
            <h2 className="font-semibold text-base">{t('tools.title')}</h2>
            <p className="mt-1 text-muted-foreground text-sm">{t('tools.description')}</p>
          </div>
        </div>
        <Badge className="mt-0.5" variant="secondary">
          {t('tools.count', { count: tools.length })}
        </Badge>
      </div>

      <div className="border-border border-b p-4">
        <div className="relative">
          <Search className="-translate-y-1/2 pointer-events-none absolute top-1/2 left-3 size-4 text-muted-foreground" />
          <Input
            aria-label={t('tools.searchLabel')}
            className="pl-9"
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t('tools.searchPlaceholder')}
            value={query}
          />
        </div>
      </div>

      {toolsQuery.isLoading ? (
        <p className="p-5 text-muted-foreground text-sm">{t('tools.loading')}</p>
      ) : toolsQuery.isError ? (
        <p className="p-5 text-destructive text-sm">{t('tools.error')}</p>
      ) : filteredTools.length === 0 ? (
        <p className="p-5 text-muted-foreground text-sm">{t('tools.empty')}</p>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full min-w-[960px] border-collapse text-left text-sm">
            <thead className="bg-background text-muted-foreground">
              <tr className="border-border border-b">
                <th className="px-5 py-3 font-medium">{t('tools.columns.tool')}</th>
                <th className="px-5 py-3 font-medium">{t('tools.columns.group')}</th>
                <th className="px-5 py-3 font-medium">{t('tools.columns.origin')}</th>
                <th className="px-5 py-3 font-medium">{t('tools.columns.access')}</th>
                <th className="px-5 py-3 font-medium">{t('tools.columns.execution')}</th>
                <th className="px-5 py-3 font-medium">{t('tools.columns.description')}</th>
              </tr>
            </thead>
            <tbody>
              {filteredTools.map((tool) => {
                const groupLabel = groupLabelForTool(tool)
                return (
                  <tr className="border-border border-b last:border-b-0" key={tool.name}>
                    <td className="px-5 py-3 align-top">
                      <div className="font-medium text-foreground">{tool.displayName}</div>
                      {tool.name !== tool.displayName ? (
                        <div className="mt-0.5 font-mono text-muted-foreground text-xs">
                          {tool.name}
                        </div>
                      ) : null}
                    </td>
                    <td className="px-5 py-3 align-top text-muted-foreground">{groupLabel}</td>
                    <td className="px-5 py-3 align-top">
                      <div className="text-muted-foreground">
                        {t(`tools.origin.${tool.originKind}`)}
                      </div>
                      {tool.originId ? (
                        <div className="mt-0.5 max-w-40 truncate font-mono text-muted-foreground text-xs">
                          {tool.originId}
                        </div>
                      ) : null}
                    </td>
                    <td className="px-5 py-3 align-top">
                      <Badge variant={accessBadgeVariant(tool.access)}>
                        {t(`tools.access.${tool.access}`)}
                      </Badge>
                    </td>
                    <td className="px-5 py-3 align-top text-muted-foreground">
                      {t(`tools.execution.${tool.executionChannel}`)}
                    </td>
                    <td className="max-w-md px-5 py-3 align-top text-muted-foreground">
                      {tool.description}
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </div>
      )}
    </section>
  )
}

function accessBadgeVariant(access: RuntimeToolSummary['access']) {
  if (access === 'destructive') {
    return 'destructive'
  }
  if (access === 'readOnly') {
    return 'secondary'
  }
  return 'outline'
}
