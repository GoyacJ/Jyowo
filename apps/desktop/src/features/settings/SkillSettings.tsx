import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  ChevronLeft,
  ChevronRight,
  FileText,
  Folder,
  Search,
  Trash2,
  Upload,
  Wrench,
} from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  deleteSkill,
  getSkill,
  importSkill,
  listSkills,
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
import { Switch } from '@/shared/ui/switch'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/shared/ui/tabs'

import { MCPManager } from './MCPManager'

type BuiltinTool = {
  name: string
  displayName: string
  group: string
  access: 'readOnly' | 'mutating' | 'destructive'
}

const BUILTIN_TOOLS: BuiltinTool[] = [
  { name: 'FileRead', displayName: 'File read', group: 'fileSystem', access: 'readOnly' },
  { name: 'FileEdit', displayName: 'File edit', group: 'fileSystem', access: 'destructive' },
  { name: 'FileWrite', displayName: 'File write', group: 'fileSystem', access: 'destructive' },
  { name: 'ListDir', displayName: 'List directory', group: 'fileSystem', access: 'readOnly' },
  { name: 'Grep', displayName: 'Grep', group: 'search', access: 'readOnly' },
  { name: 'Glob', displayName: 'Glob', group: 'search', access: 'readOnly' },
  { name: 'ReadBlob', displayName: 'Read blob', group: 'meta', access: 'readOnly' },
  { name: 'Bash', displayName: 'Bash', group: 'shell', access: 'destructive' },
  { name: 'WebFetch', displayName: 'Web fetch', group: 'network', access: 'readOnly' },
  { name: 'WebSearch', displayName: 'Web search', group: 'network', access: 'readOnly' },
  { name: 'Clarify', displayName: 'Clarify', group: 'clarification', access: 'mutating' },
  { name: 'SendMessage', displayName: 'Send message', group: 'network', access: 'mutating' },
  { name: 'Todo', displayName: 'Todo', group: 'memory', access: 'mutating' },
  { name: 'TaskStop', displayName: 'Task stop', group: 'agent', access: 'mutating' },
  { name: 'skills_list', displayName: 'List skills', group: 'meta', access: 'readOnly' },
  { name: 'skills_view', displayName: 'View skill', group: 'meta', access: 'readOnly' },
  { name: 'skills_invoke', displayName: 'Invoke skill', group: 'meta', access: 'readOnly' },
]

const skillQueryKeys = {
  all: ['skills'] as const,
  detail: (id: string | null, selectedFilePath: string | null) =>
    [...skillQueryKeys.all, 'detail', id, selectedFilePath] as const,
  list: () => [...skillQueryKeys.all, 'list'] as const,
}

const SKILLS_PAGE_SIZE = 8

type SkillStatusFilter = 'all' | SkillSummary['status']
type SkillSourceFilter = 'all' | SkillSummary['sourceKind']

function useSkills() {
  const commandClient = useCommandClient()

  return useQuery({
    queryKey: skillQueryKeys.list(),
    queryFn: () => listSkills(commandClient),
  })
}

function useSkillDetail(id: string | null, selectedFilePath: string | null) {
  const commandClient = useCommandClient()

  return useQuery({
    enabled: id !== null,
    queryKey: skillQueryKeys.detail(id, selectedFilePath),
    queryFn: () => getSkill(id ?? '', true, commandClient, selectedFilePath ?? undefined),
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

export function SkillSettingsPage() {
  const { t } = useTranslation('skills')

  return (
    <section aria-label={t('pageTitle')} className="h-full min-h-0 overflow-y-auto pr-1">
      <div className="mx-auto flex w-full max-w-5xl flex-col gap-3 pb-6">
        <Tabs className="min-h-0" defaultValue="skills">
          <TabsList aria-label={t('tabs.label')}>
            <TabsTrigger value="skills">{t('tabs.skills')}</TabsTrigger>
            <TabsTrigger value="tools">{t('tabs.tools')}</TabsTrigger>
            <TabsTrigger value="mcp">{t('tabs.mcp')}</TabsTrigger>
          </TabsList>

          <TabsContent className="space-y-5 pt-3" value="skills">
            <SkillsManager />
          </TabsContent>
          <TabsContent className="space-y-5 pt-3" value="tools">
            <BuiltinToolsList />
          </TabsContent>
          <TabsContent className="space-y-5 pt-3" value="mcp">
            <MCPManager />
          </TabsContent>
        </Tabs>
      </div>
    </section>
  )
}

export function SkillsManager() {
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
  const detailQuery = useSkillDetail(selectedId, selectedFilePath)
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
        <label className="relative block text-sm">
          <span className="sr-only">{t('filters.search')}</span>
          <Search className="absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
          <input
            className="h-10 w-full rounded-md border border-border bg-background pl-9 pr-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
            onChange={(event) => updateSearch(event.target.value)}
            placeholder={t('filters.search')}
            value={search}
          />
        </label>
        <label className="block text-sm">
          <span className="sr-only">{t('filters.status')}</span>
          <select
            className="h-10 w-full rounded-md border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
            onChange={(event) => updateStatusFilter(event.target.value as SkillStatusFilter)}
            value={statusFilter}
          >
            <option value="all">{t('filters.allStatuses')}</option>
            <option value="ready">{t('status.ready')}</option>
            <option value="disabled">{t('status.disabled')}</option>
            <option value="prerequisite_missing">{t('status.prerequisite_missing')}</option>
            <option value="rejected">{t('status.rejected')}</option>
          </select>
        </label>
        <label className="block text-sm">
          <span className="sr-only">{t('filters.source')}</span>
          <select
            className="h-10 w-full rounded-md border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
            onChange={(event) => updateSourceFilter(event.target.value as SkillSourceFilter)}
            value={sourceFilter}
          >
            <option value="all">{t('filters.allSources')}</option>
            <option value="workspace">{t('source.workspace')}</option>
            <option value="user">{t('source.user')}</option>
            <option value="bundled">{t('source.bundled')}</option>
            <option value="plugin">{t('source.plugin')}</option>
            <option value="mcp">{t('source.mcp')}</option>
          </select>
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
            onSelectFile={setSelectedFilePath}
            selectedFilePath={detailQuery.data?.skill.selectedFile?.path ?? selectedFilePath}
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

function SkillListItem({
  deletePending,
  onDelete,
  onSelect,
  onToggle,
  selected,
  skill,
  togglePending,
}: {
  deletePending: boolean
  onDelete: (skill: SkillSummary) => void
  onSelect: (id: string) => void
  onToggle: (skill: SkillSummary) => void
  selected: boolean
  skill: SkillSummary
  togglePending: boolean
}) {
  const { t } = useTranslation('skills')

  return (
    <div
      className="rounded-md border border-border bg-surface px-2.5 py-2 text-sm transition-colors data-[selected=true]:border-primary data-[selected=true]:bg-muted/35"
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
      ) : null}
    </div>
  )
}

function SkillFilesTab({
  detailQuery,
  onSelectFile,
  selectedFilePath,
  selectedSkill,
}: {
  detailQuery: ReturnType<typeof useSkillDetail>
  onSelectFile: (path: string) => void
  selectedFilePath: string | null
  selectedSkill: SkillSummary | null
}) {
  const { t } = useTranslation('skills')
  const files = detailQuery.data?.skill.files ?? []
  const selectedFile = detailQuery.data?.skill.selectedFile
  const detail = detailQuery.data?.skill ?? null

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
            {selectedFile?.content ?? detail?.bodyFull ?? detail?.bodyPreview ?? t('content.empty')}
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
  onSelectFile,
  selectedFilePath,
  selectedSkill,
}: {
  detailQuery: ReturnType<typeof useSkillDetail>
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

export function BuiltinToolsList() {
  const { t } = useTranslation('skills')

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
          {t('tools.count', { count: BUILTIN_TOOLS.length })}
        </Badge>
      </div>

      <div className="overflow-x-auto">
        <table className="w-full min-w-[720px] border-collapse text-left text-sm">
          <thead className="bg-background text-muted-foreground">
            <tr className="border-border border-b">
              <th className="px-5 py-3 font-medium">{t('tools.columns.tool')}</th>
              <th className="px-5 py-3 font-medium">{t('tools.columns.group')}</th>
              <th className="px-5 py-3 font-medium">{t('tools.columns.access')}</th>
              <th className="px-5 py-3 font-medium">{t('tools.columns.description')}</th>
            </tr>
          </thead>
          <tbody>
            {BUILTIN_TOOLS.map((tool) => (
              <tr className="border-border border-b last:border-b-0" key={tool.name}>
                <td className="px-5 py-3 align-top">
                  <div className="font-medium text-foreground">{tool.displayName}</div>
                  {tool.name !== tool.displayName ? (
                    <div className="mt-0.5 font-mono text-muted-foreground text-xs">
                      {tool.name}
                    </div>
                  ) : null}
                </td>
                <td className="px-5 py-3 align-top text-muted-foreground">
                  {t(`tools.groups.${tool.group}`)}
                </td>
                <td className="px-5 py-3 align-top">
                  <Badge variant={accessBadgeVariant(tool.access)}>
                    {t(`tools.access.${tool.access}`)}
                  </Badge>
                </td>
                <td className="max-w-md px-5 py-3 align-top text-muted-foreground">
                  {t(`tools.items.${tool.name}.description`)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  )
}

function accessBadgeVariant(access: BuiltinTool['access']) {
  if (access === 'destructive') {
    return 'destructive'
  }
  if (access === 'readOnly') {
    return 'secondary'
  }
  return 'outline'
}
