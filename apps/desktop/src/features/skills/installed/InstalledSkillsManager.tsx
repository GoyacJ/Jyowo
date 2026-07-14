import { ChevronLeft, ChevronRight, ExternalLink, Search, Trash2, Upload } from 'lucide-react'
import { type ReactNode, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  useDeleteSkill,
  useImportSkill,
  useSetSkillEnabled,
  useSkillDetail,
  useSkillFile,
  useSkills,
} from '@/features/skills/api/queries'
import { SkillFileTree } from '@/features/skills/components/SkillFileTree'
import type { SkillSummary } from '@/shared/tauri/commands'
import { getCommandErrorMessage } from '@/shared/tauri/errors'
import { pickSkillPackagePath } from '@/shared/tauri/file-dialog'
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

const SKILLS_PAGE_SIZE = 8

type SkillStatusFilter = 'all' | SkillSummary['status']
type SkillSourceFilter = 'all' | SkillSummary['sourceKind']

export type SkillConfigRenderer = (skillId: string) => ReactNode

export function InstalledSkillsManager({
  onOpenPlugin,
  renderConfig,
}: {
  onOpenPlugin?: (pluginId: string) => void
  renderConfig?: SkillConfigRenderer
}) {
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
  const [mutationError, setMutationError] = useState<string | null>(null)
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
    setMutationError(null)
    try {
      const selected = await pickSkillPackagePath()
      if (selected !== null) {
        await importMutation.mutateAsync(selected)
      }
    } catch (error) {
      setMutationError(getCommandErrorMessage(error))
    }
  }

  function selectSkill(id: string) {
    setSelectedId(id)
    setSelectedFilePath(null)
    setMutationError(null)
  }

  async function toggleSkill(skill: SkillSummary) {
    if (!skill.manageable) {
      return
    }
    setMutationError(null)
    try {
      await setEnabledMutation.mutateAsync({ enabled: !skill.enabled, id: skill.id })
    } catch (error) {
      setMutationError(getCommandErrorMessage(error))
    }
  }

  async function removeSkill(skill: SkillSummary) {
    if (!skill.manageable) {
      return
    }
    setMutationError(null)
    try {
      await deleteMutation.mutateAsync(skill.id)
      setPendingDeleteSkill(null)
      if (selectedId === skill.id) {
        setSelectedId(null)
        setSelectedFilePath(null)
      }
    } catch (error) {
      setMutationError(getCommandErrorMessage(error))
    }
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
          onClick={() => void pickAndImportSkill()}
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
      {mutationError ? (
        <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
          {mutationError}
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
                  onDelete={setPendingDeleteSkill}
                  onOpenPlugin={onOpenPlugin}
                  onSelect={selectSkill}
                  onToggle={(value) => void toggleSkill(value)}
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
            renderConfig={renderConfig}
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
            {getCommandErrorMessage(detailQuery.error)}
          </div>
        ) : null}
        {selectedSkill && !detailQuery.isLoading && !detailQuery.isError && files.length === 0 ? (
          <div className="p-3 text-muted-foreground text-sm">{t('files.noFiles')}</div>
        ) : null}
        {files.length > 0 ? (
          <div className="max-h-[560px] overflow-y-auto p-2">
            <SkillFileTree
              files={files}
              onSelectFile={onSelectFile}
              selectedFilePath={selectedFilePath}
            />
          </div>
        ) : null}
      </div>
      <div className="min-w-0 rounded-md border border-border bg-surface">
        <div className="border-border border-b px-3 py-2 font-medium text-sm">
          {selectedFile?.path ?? t('content.title')}
        </div>
        {fileQuery.isError ? (
          <div className="m-3 rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
            {getCommandErrorMessage(fileQuery.error)}
          </div>
        ) : (
          <pre className="max-h-[560px] min-h-[360px] max-w-full overflow-auto p-3 text-sm leading-6">
            <code className="block min-w-full w-max whitespace-pre">
              {fileQuery.isLoading
                ? t('files.loading')
                : (selectedFile?.content ?? detail?.bodyPreview ?? t('content.empty'))}
            </code>
          </pre>
        )}
      </div>
    </section>
  )
}

function SkillDetailPanel({
  detailQuery,
  fileQuery,
  onSelectFile,
  renderConfig,
  selectedFilePath,
  selectedSkill,
}: {
  detailQuery: ReturnType<typeof useSkillDetail>
  fileQuery: ReturnType<typeof useSkillFile>
  onSelectFile: (path: string) => void
  renderConfig?: SkillConfigRenderer
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
              <Badge variant={summary.manageable ? 'secondary' : 'outline'}>
                {summary.manageable ? t('manageable') : t('readOnly')}
              </Badge>
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
                <OverviewItem label={t('filters.status')} value={t(`status.${summary.status}`)} />
                <OverviewItem
                  label={t('filters.source')}
                  value={t(`source.${summary.sourceKind}`)}
                />
                <OverviewItem label={t('detail.importedAt')} value={summary.importedAt ?? '-'} />
                <OverviewItem label={t('detail.updatedAt')} value={summary.updatedAt ?? '-'} />
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
                detail.parameters.length === 0 ? (
                  <p className="text-muted-foreground text-sm">{t('detail.noParameters')}</p>
                ) : (
                  <div className="overflow-x-auto rounded-md border border-border">
                    <table className="w-full min-w-[520px] text-left text-sm">
                      <thead className="bg-muted text-muted-foreground">
                        <tr>
                          <th className="px-3 py-2 font-medium">{t('detail.paramName')}</th>
                          <th className="px-3 py-2 font-medium">{t('detail.paramType')}</th>
                          <th className="px-3 py-2 font-medium">{t('detail.paramRequired')}</th>
                          <th className="px-3 py-2 font-medium">{t('detail.paramDescription')}</th>
                        </tr>
                      </thead>
                      <tbody>
                        {detail.parameters.map((parameter) => (
                          <tr className="border-border border-t" key={parameter.name}>
                            <td className="px-3 py-2 font-mono">{parameter.name}</td>
                            <td className="px-3 py-2">{parameter.paramType}</td>
                            <td className="px-3 py-2">{parameter.required ? t('yes') : t('no')}</td>
                            <td className="px-3 py-2 text-muted-foreground">
                              {parameter.description ?? ''}
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                )
              ) : null}
            </TabsContent>
            <TabsContent className="pt-2" value="config">
              {detailQuery.isLoading ? (
                <div className="text-muted-foreground text-sm">{t('detail.loading')}</div>
              ) : null}
              {detail ? (
                renderConfig ? (
                  renderConfig(summary.id)
                ) : detail.configKeys.length === 0 ? (
                  <p className="text-muted-foreground text-sm">{t('detail.noConfigKeys')}</p>
                ) : (
                  <div className="flex flex-wrap gap-1.5">
                    {detail.configKeys.map((key) => (
                      <Badge className="font-mono" key={key} variant="outline">
                        {key}
                      </Badge>
                    ))}
                  </div>
                )
              ) : null}
            </TabsContent>
          </Tabs>
        </div>
      ) : null}
    </section>
  )
}

function OverviewItem({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md border border-border bg-surface p-3">
      <div className="text-muted-foreground">{label}</div>
      <div className="mt-1 font-medium">{value}</div>
    </div>
  )
}
