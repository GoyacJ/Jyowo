import { useQueryClient } from '@tanstack/react-query'
import { ChevronLeft, ChevronRight, Download, Search } from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  type SkillCatalogSourceId,
  skillQueryKeys,
  useInstallSkillFromCatalog,
  useSkillCatalogEntries,
  useSkillCatalogEntry,
  useSkillCatalogFile,
  useSkillCatalogInstallTasks,
  useSkillCatalogSources,
} from '@/features/skills/api/queries'
import {
  catalogInstallTaskFromProgress,
  findCatalogInstallTask,
  isTerminalCatalogInstallTask,
  reduceCatalogInstallTask,
} from '@/features/skills/components/catalog-task-reducer'
import { buildSkillFileTree, SkillFileTree } from '@/features/skills/components/SkillFileTree'
import {
  type GetSkillCatalogFileResponse,
  listenSkillCatalogInstallProgress,
  type SkillCatalogEntry,
  type SkillCatalogInstallProgressPayload,
  type SkillCatalogInstallTask,
  type SkillCatalogSource,
} from '@/shared/tauri/commands'
import { getCommandErrorMessage } from '@/shared/tauri/errors'
import { useCommandClient } from '@/shared/tauri/react'
import { Badge } from '@/shared/ui/badge'
import { Button } from '@/shared/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/shared/ui/dialog'
import { Input } from '@/shared/ui/input'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/shared/ui/tooltip'

export function SkillCatalogManager() {
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
  const [catalogInstallError, setCatalogInstallError] = useState<string | null>(null)
  const [detailOpen, setDetailOpen] = useState(false)
  const observedTerminalOperations = useRef(new Set<string>())
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
    () => buildSkillFileTree(detailQuery.data?.files ?? []),
    [detailQuery.data?.files],
  )
  const selectedCatalogInstallTask = findCatalogInstallTask(catalogInstallTasks, selectedEntry)
  const selectedCatalogInstallInFlight = selectedCatalogInstallTask?.status === 'running'
  const catalogInstallDisabled =
    installMutation.isPending ||
    selectedCatalogInstallInFlight ||
    detailQuery.isLoading ||
    !detailQuery.data ||
    detailEntry?.installable === false ||
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
    const unseenTerminalTasks = catalogInstallTasks.filter(
      (task) =>
        isTerminalCatalogInstallTask(task) &&
        !observedTerminalOperations.current.has(task.operationId),
    )
    if (unseenTerminalTasks.length === 0) {
      return
    }
    for (const task of unseenTerminalTasks) {
      observedTerminalOperations.current.add(task.operationId)
    }
    void refreshInstalledAndCatalog(queryClient)
  }, [catalogInstallTasks, queryClient])

  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | null = null

    const onProgress = (progress: SkillCatalogInstallProgressPayload) => {
      try {
        const current = queryClient.getQueryData<{ tasks: SkillCatalogInstallTask[] }>(
          skillQueryKeys.catalogInstallTasks(),
        )
        const existing =
          current?.tasks.find((task) => task.operationId === progress.operationId) ?? null
        const nextTask = catalogInstallTaskFromProgress(progress, existing)
        queryClient.setQueryData(skillQueryKeys.catalogInstallTasks(), (state) =>
          reduceCatalogInstallTask(
            state as { tasks: SkillCatalogInstallTask[] } | undefined,
            nextTask,
          ),
        )
        if (
          isTerminalCatalogInstallTask(nextTask) &&
          !observedTerminalOperations.current.has(nextTask.operationId)
        ) {
          observedTerminalOperations.current.add(nextTask.operationId)
          void refreshInstalledAndCatalog(queryClient)
        }
      } catch (error) {
        setCatalogInstallError(getCommandErrorMessage(error))
      }
    }

    listenSkillCatalogInstallProgress(onProgress, commandClient, (error) => {
      setCatalogInstallError(getCommandErrorMessage(error))
    })
      .then((cleanup) => {
        if (disposed) {
          cleanup()
        } else {
          unlisten = cleanup
        }
      })
      .catch((error: unknown) => {
        setCatalogInstallError(getCommandErrorMessage(error))
      })

    return () => {
      disposed = true
      unlisten?.()
    }
  }, [commandClient, queryClient])

  function resetCatalogSelection() {
    setSelectedEntry(null)
    setSelectedCatalogFilePath(null)
    setCatalogInstallError(null)
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
    setCatalogInstallError(null)
    installMutation.reset()
    setDetailOpen(true)
  }

  function goToNextPage() {
    const nextCursor = entriesQuery.data?.nextCursor
    if (nextCursor) {
      setCursorHistory((current) => [...current, cursor])
      setCursor(nextCursor)
    }
  }

  function goToPreviousPage() {
    setCursorHistory((current) => {
      setCursor(current.at(-1) ?? null)
      return current.slice(0, -1)
    })
  }

  function catalogSourceLabel(source: SkillCatalogSource) {
    return t(`catalog.sourcesById.${source.id}.label`, { defaultValue: source.label })
  }

  function catalogSourceDescription(source: SkillCatalogSource) {
    return t(`catalog.sourcesById.${source.id}.description`, { defaultValue: source.description })
  }

  function catalogSourceLabelById(entry: SkillCatalogEntry) {
    const source = sourcesQuery.data?.sources.find((candidate) => candidate.id === entry.sourceId)
    return source ? catalogSourceLabel(source) : entry.sourceLabel
  }

  async function installSelectedEntry() {
    if (selectedEntry === null || catalogInstallDisabled) {
      return
    }

    const entry = selectedEntry
    const operationId = createCatalogInstallOperationId()
    const startedAt = new Date().toISOString()
    const optimisticTask = {
      entryId: entry.entryId,
      operationId,
      percent: 5,
      sourceId: entry.sourceId,
      stage: 'preparing',
      startedAt,
      status: 'running',
      updatedAt: startedAt,
      version: entry.version,
    } satisfies SkillCatalogInstallTask
    queryClient.setQueryData(skillQueryKeys.catalogInstallTasks(), (state) =>
      reduceCatalogInstallTask(
        state as { tasks: SkillCatalogInstallTask[] } | undefined,
        optimisticTask,
      ),
    )
    setCatalogInstallError(null)

    try {
      const response = await installMutation.mutateAsync({ entry, operationId })
      queryClient.setQueryData(skillQueryKeys.catalogInstallTasks(), (state) =>
        reduceCatalogInstallTask(
          state as { tasks: SkillCatalogInstallTask[] } | undefined,
          response.task,
        ),
      )
      if (isTerminalCatalogInstallTask(response.task)) {
        observedTerminalOperations.current.add(response.task.operationId)
        await refreshInstalledAndCatalog(queryClient)
      }
    } catch (error) {
      const message = getCommandErrorMessage(error)
      const failedAt = new Date().toISOString()
      const failedTask = {
        ...optimisticTask,
        message,
        percent: 100,
        stage: 'failed',
        status: 'failed',
        updatedAt: failedAt,
      } satisfies SkillCatalogInstallTask
      queryClient.setQueryData(skillQueryKeys.catalogInstallTasks(), (state) =>
        reduceCatalogInstallTask(
          state as { tasks: SkillCatalogInstallTask[] } | undefined,
          failedTask,
        ),
      )
      setCatalogInstallError(message)
    }
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
          {sourcesQuery.isError ? <ErrorState message={t('catalog.sourceError')} /> : null}
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
          {entriesQuery.isError ? <ErrorState message={t('catalog.entriesError')} /> : null}
          {catalogInstallTasksQuery.isError ? (
            <ErrorState message={getCommandErrorMessage(catalogInstallTasksQuery.error)} />
          ) : null}
          {catalogInstallError ? <ErrorState message={catalogInstallError} /> : null}
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
          if (!open) setSelectedEntry(null)
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
                    <SkillFileTree
                      files={catalogFiles}
                      onSelectFile={setSelectedCatalogFilePath}
                      selectedFilePath={selectedCatalogFilePath}
                    />
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
                        onClick={() => void installSelectedEntry()}
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
                            {t('catalog.install')}
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
                      <ErrorState message={getCommandErrorMessage(detailQuery.error)} />
                    ) : null}
                    {catalogInstallError ? <ErrorState message={catalogInstallError} /> : null}
                    {selectedCatalogInstallTask?.status === 'failed' && !catalogInstallError ? (
                      <ErrorState
                        message={selectedCatalogInstallTask.message ?? t('catalog.installError')}
                      />
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
            {catalogFileErrorMessage(fileQuery.error, t('catalog.filePreviewUnavailable'))}
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

function ErrorState({ message }: { message: string }) {
  return (
    <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
      {message}
    </div>
  )
}

function catalogFileErrorMessage(error: unknown, previewUnavailable: string) {
  const message = getCommandErrorMessage(error)
  return message === 'catalog text file must be valid UTF-8' ? previewUnavailable : message
}

function createCatalogInstallOperationId() {
  const randomUUID = globalThis.crypto?.randomUUID?.()
  return randomUUID
    ? `catalog-install-${randomUUID}`
    : `catalog-install-${Date.now()}-${Math.random().toString(36).slice(2)}`
}

async function refreshInstalledAndCatalog(queryClient: ReturnType<typeof useQueryClient>) {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: skillQueryKeys.list(), refetchType: 'all' }),
    queryClient.invalidateQueries({ queryKey: skillQueryKeys.catalog(), refetchType: 'all' }),
  ])
}
