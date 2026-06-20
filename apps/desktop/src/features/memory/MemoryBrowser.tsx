import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Download, Save, Trash2 } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  exportMemoryItems,
  getMemoryItem,
  listMemoryItems,
  type UpdateMemoryItemRequest,
  updateMemoryItem,
} from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'
import { Button } from '@/shared/ui/button'

import { MemoryItemCard } from './MemoryItemCard'

const memoryQueryKeys = {
  all: ['memory'] as const,
  detail: (id: string | null) => [...memoryQueryKeys.all, 'detail', id] as const,
  list: () => [...memoryQueryKeys.all, 'list'] as const,
}

export function MemoryBrowser() {
  const { t } = useTranslation('memory')
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [draftContent, setDraftContent] = useState('')
  const [deleteCandidateId, setDeleteCandidateId] = useState<string | null>(null)
  const [exportMessage, setExportMessage] = useState<string | null>(null)

  const memoryItemsQuery = useQuery({
    queryKey: memoryQueryKeys.list(),
    queryFn: () => listMemoryItems(commandClient),
  })
  const detailQuery = useQuery({
    enabled: selectedId !== null,
    queryKey: memoryQueryKeys.detail(selectedId),
    queryFn: () => getMemoryItem(selectedId ?? '', commandClient),
  })
  const updateMutation = useMutation({
    mutationFn: (request: UpdateMemoryItemRequest) => updateMemoryItem(request, commandClient),
    onSuccess: async (response) => {
      setDraftContent(response.item.content)
      await queryClient.invalidateQueries({ queryKey: memoryQueryKeys.list() })
      await queryClient.invalidateQueries({ queryKey: memoryQueryKeys.detail(response.item.id) })
    },
  })
  const deleteMutation = useMutation({
    mutationFn: (id: string) => commandClient.deleteMemoryItem(id),
    onSuccess: async (response) => {
      setDeleteCandidateId(null)
      if (selectedId === response.id) {
        setSelectedId(null)
        setDraftContent('')
      }
      await queryClient.invalidateQueries({ queryKey: memoryQueryKeys.all })
    },
  })
  const exportMutation = useMutation({
    mutationFn: () => exportMemoryItems(commandClient),
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

    await deleteMutation.mutateAsync(deleteCandidateId)
  }

  return (
    <section className="space-y-5 rounded-md border border-border bg-background p-5">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="font-semibold text-base">{t('title')}</h2>
          <p className="mt-1 text-muted-foreground text-sm">
            {t('visibleItems', { count: items.length })}
          </p>
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
      </div>

      {memoryItemsQuery.isLoading ? (
        <div className="text-muted-foreground text-sm">{t('loading')}</div>
      ) : null}

      {memoryItemsQuery.isError ? (
        <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
          {t('loadError')}
        </div>
      ) : null}

      {!memoryItemsQuery.isLoading && !memoryItemsQuery.isError && items.length === 0 ? (
        <div className="rounded-md border border-dashed border-border bg-surface px-4 py-6 text-center text-muted-foreground text-sm">
          {t('empty')}
        </div>
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
              <section
                aria-label={t('detail')}
                className="space-y-4 rounded-md border border-border bg-surface p-4"
              >
                <div className="space-y-1 text-sm">
                  <div className="font-medium">{selectedItem.kind}</div>
                  <div className="text-muted-foreground">
                    {t('source', { source: selectedItem.source })}
                  </div>
                  <div className="text-muted-foreground">
                    {t('visibility', { visibility: selectedItem.visibility })}
                  </div>
                </div>

                <label className="block space-y-2 text-sm">
                  <span className="font-medium">{t('content')}</span>
                  <textarea
                    className="min-h-36 w-full resize-y rounded-md border border-border bg-background px-3 py-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    onChange={(event) => setDraftContent(event.target.value)}
                    value={draftContent}
                  />
                </label>

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
              </section>
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
    </section>
  )
}
