import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Download, Save, Trash2 } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useDaemonClient } from '@/shared/tauri/react'
import { Button } from '@/shared/ui/button'
import { Card, CardContent } from '@/shared/ui/card'
import { EmptyState } from '@/shared/ui/empty-state'
import { FieldControl } from '@/shared/ui/field'
import { Section, SectionDescription, SectionHeader, SectionTitle } from '@/shared/ui/section'
import { Textarea } from '@/shared/ui/textarea'

import { MemoryItemCard } from './MemoryItemCard'
import type { DeleteMemoryItemRequest, UpdateMemoryItemRequest } from './memory-types'

const memoryQueryKeys = {
  all: ['memory'] as const,
  detail: (workspaceRoot: string | undefined, id: string | null) =>
    [...memoryQueryKeys.all, workspaceRoot ?? null, 'detail', id] as const,
  list: (workspaceRoot: string | undefined) =>
    [...memoryQueryKeys.all, workspaceRoot ?? null, 'list'] as const,
}

export function MemoryBrowser({ workspaceRoot }: { workspaceRoot?: string }) {
  const { t } = useTranslation('memory')
  const daemonClient = useDaemonClient()
  const queryClient = useQueryClient()
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [draftContent, setDraftContent] = useState('')
  const [deleteCandidateId, setDeleteCandidateId] = useState<string | null>(null)
  const [exportMessage, setExportMessage] = useState<string | null>(null)

  const memoryItemsQuery = useQuery({
    queryKey: memoryQueryKeys.list(workspaceRoot),
    queryFn: () => daemonClient.listMemoryItems(workspaceRoot),
  })
  const detailQuery = useQuery({
    enabled: selectedId !== null,
    queryKey: memoryQueryKeys.detail(workspaceRoot, selectedId),
    queryFn: () => daemonClient.getMemoryItem(workspaceRoot, selectedId ?? ''),
  })
  const updateMutation = useMutation({
    mutationFn: (request: UpdateMemoryItemRequest) =>
      daemonClient.updateMemoryItem(workspaceRoot, request),
    onSuccess: async (response) => {
      setDraftContent(response.item.content)
      await queryClient.invalidateQueries({ queryKey: memoryQueryKeys.list(workspaceRoot) })
      await queryClient.invalidateQueries({
        queryKey: memoryQueryKeys.detail(workspaceRoot, response.item.id),
      })
    },
  })
  const deleteMutation = useMutation({
    mutationFn: (request: DeleteMemoryItemRequest) =>
      daemonClient.deleteMemoryItem(workspaceRoot, request),
    onSuccess: async (response) => {
      setDeleteCandidateId(null)
      if (selectedId === response.memoryId) {
        setSelectedId(null)
        setDraftContent('')
      }
      await queryClient.invalidateQueries({ queryKey: memoryQueryKeys.all })
    },
  })
  const exportMutation = useMutation({
    mutationFn: () =>
      daemonClient.exportMemoryItems(workspaceRoot, {
        explicitUserAction: true,
        format: 'json',
        includeHashes: true,
        includeMetadata: true,
        includeRawContent: false,
        scope: 'visible',
      }),
    onSuccess: (response) => {
      setExportMessage(t('exportSaved', { count: response.itemCount, path: response.path }))
    },
  })

  const items = memoryItemsQuery.data?.items ?? []
  const selectedItem = detailQuery.data?.item ?? null

  useEffect(() => {
    if (selectedItem) {
      setDraftContent(selectedItem.content)
    }
  }, [selectedItem])

  function inspectMemoryItem(id: string) {
    setSelectedId(id)
    setDeleteCandidateId(null)
    setExportMessage(null)
  }

  async function saveMemoryItem() {
    if (!selectedId || draftContent.trim().length === 0) {
      return
    }

    await updateMutation.mutateAsync({
      content: draftContent,
      id: selectedId,
    })
  }

  async function confirmDelete() {
    if (!deleteCandidateId) {
      return
    }

    await deleteMutation.mutateAsync({ id: deleteCandidateId })
  }

  return (
    <Section>
      <SectionHeader className="flex items-start justify-between gap-3">
        <div>
          <SectionTitle>{t('title')}</SectionTitle>
          <SectionDescription>{t('visibleItems', { count: items.length })}</SectionDescription>
        </div>
        <Button
          disabled={exportMutation.isPending}
          onClick={() => exportMutation.mutate()}
          type="button"
          variant="outline"
        >
          <Download data-icon className="size-4" />
          {t('export')}
        </Button>
      </SectionHeader>

      {memoryItemsQuery.isLoading ? (
        <div className="text-muted-foreground text-sm">{t('loading')}</div>
      ) : null}

      {memoryItemsQuery.isError ? (
        <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
          {t('loadError')}
        </div>
      ) : null}

      {!memoryItemsQuery.isLoading && !memoryItemsQuery.isError && items.length === 0 ? (
        <EmptyState>{t('empty')}</EmptyState>
      ) : null}

      {exportMutation.isError ? (
        <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
          {t('exportError')}
        </div>
      ) : null}

      {exportMessage ? (
        <div className="rounded-md border border-border bg-surface px-3 py-2 text-sm">
          {exportMessage}
        </div>
      ) : null}

      {items.length > 0 ? (
        <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(320px,420px)]">
          <nav aria-label={t('title')} className="max-h-[620px] space-y-3 overflow-y-auto pr-1">
            {items.map((item) => (
              <MemoryItemCard
                item={item}
                key={item.id}
                onDelete={setDeleteCandidateId}
                onInspect={inspectMemoryItem}
              />
            ))}
          </nav>

          <aside className="space-y-3">
            {detailQuery.isLoading ? (
              <div className="text-muted-foreground text-sm">{t('detailLoading')}</div>
            ) : null}

            {detailQuery.isError ? (
              <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
                {t('detailLoadError')}
              </div>
            ) : null}

            {selectedItem ? (
              <Card aria-label={t('detail')} role="region">
                <CardContent className="space-y-4 pt-4">
                  <div className="space-y-1 text-sm">
                    <div className="font-medium">{selectedItem.kind}</div>
                    <div className="text-muted-foreground">
                      {t('source', { source: selectedItem.source })}
                    </div>
                    <div className="text-muted-foreground">
                      {t('visibility', { visibility: selectedItem.visibility })}
                    </div>
                    {selectedItem.providerId ? (
                      <div className="text-muted-foreground">
                        {t('provider', { provider: selectedItem.providerId })}
                      </div>
                    ) : null}
                    <div className="break-all text-muted-foreground">
                      {t('contentHash', { value: selectedItem.contentHash })}
                    </div>
                    {selectedItem.expiresAt ? (
                      <div className="text-muted-foreground">
                        {t('expiresAt', { value: selectedItem.expiresAt })}
                      </div>
                    ) : null}
                    {selectedItem.lastAccessedAt ? (
                      <div className="text-muted-foreground">
                        {t('lastAccessedAt', { value: selectedItem.lastAccessedAt })}
                      </div>
                    ) : null}
                    {selectedItem.deleted ? (
                      <div className="text-muted-foreground">{t('deleted')}</div>
                    ) : null}
                  </div>

                  <FieldControl fieldId="memory-content" label={t('content')}>
                    <Textarea
                      className="min-h-36"
                      id="memory-content"
                      onChange={(event) => setDraftContent(event.target.value)}
                      value={draftContent}
                    />
                  </FieldControl>

                  <div className="flex flex-wrap justify-end gap-2">
                    <Button
                      disabled={updateMutation.isPending || draftContent.trim().length === 0}
                      onClick={saveMemoryItem}
                      type="button"
                    >
                      <Save data-icon className="size-4" />
                      {t('save')}
                    </Button>
                  </div>
                </CardContent>
              </Card>
            ) : null}

            {deleteCandidateId ? (
              <div className="space-y-3 rounded-md border border-destructive/30 bg-destructive/5 p-4">
                <p className="text-sm">{t('deletePrompt', { id: deleteCandidateId })}</p>
                <div className="flex justify-end gap-2">
                  <Button
                    onClick={() => setDeleteCandidateId(null)}
                    type="button"
                    variant="outline"
                  >
                    {t('cancel')}
                  </Button>
                  <Button
                    disabled={deleteMutation.isPending}
                    onClick={confirmDelete}
                    type="button"
                    variant="destructive"
                  >
                    <Trash2 data-icon className="size-4" />
                    {t('confirmDelete')}
                  </Button>
                </div>
              </div>
            ) : null}

            {updateMutation.isError ? (
              <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
                {t('saveError')}
              </div>
            ) : null}

            {deleteMutation.isError ? (
              <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
                {t('deleteError')}
              </div>
            ) : null}
          </aside>
        </div>
      ) : null}
    </Section>
  )
}
