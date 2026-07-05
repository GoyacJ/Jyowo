import { Copy, FileText } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/shared/lib/utils'
import type { ChangeSetFile } from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'

export function DiffPane({
  conversationId,
  files,
}: {
  conversationId: string
  files: ChangeSetFile[]
  onChangeSetClick?: () => void
}) {
  const { t } = useTranslation('conversation')
  const commandClient = useCommandClient()
  const [selectedFileIndex, setSelectedFileIndex] = useState(0)
  const [copyingPatchRef, setCopyingPatchRef] = useState<string | null>(null)
  const [copyError, setCopyError] = useState(false)
  const [patchPagesByRef, setPatchPagesByRef] = useState<
    Record<string, { patch: string; truncated: boolean }>
  >({})
  const selectedFile = files[selectedFileIndex]
  const selectedPatchPage = selectedFile?.fullPatchRef
    ? patchPagesByRef[selectedFile.fullPatchRef]
    : undefined
  const isBinaryOrGenerated =
    selectedFile?.riskFlags?.some((f) => f === 'binary' || f === 'generated') ?? false

  const copySelectedFile = async () => {
    if (!selectedFile) {
      return
    }
    if (selectedFile.fullPatchRef) {
      setCopyingPatchRef(selectedFile.fullPatchRef)
      setCopyError(false)
      try {
        const response = await commandClient.getConversationDiffPatch({
          conversationId,
          fullPatchRef: selectedFile.fullPatchRef,
        })
        if (!navigator.clipboard) throw new Error('Clipboard unavailable')
        await navigator.clipboard.writeText(response.patch)
      } catch {
        setCopyError(true)
      } finally {
        setCopyingPatchRef(null)
      }
      return
    }
    if (selectedFile.preview) {
      try {
        setCopyError(false)
        if (!navigator.clipboard) throw new Error('Clipboard unavailable')
        await navigator.clipboard.writeText(selectedFile.preview)
      } catch {
        setCopyError(true)
      }
    }
  }

  return (
    <div className="flex h-full flex-col">
      {/* File list */}
      <div className="flex flex-wrap gap-1 border-border border-b p-2">
        {files.map((file, index) => (
          <button
            className={cn(
              'rounded px-2 py-1 font-mono text-xs transition-colors hover:bg-muted',
              index === selectedFileIndex ? 'bg-muted text-foreground' : 'text-muted-foreground',
            )}
            key={file.path}
            onClick={() => setSelectedFileIndex(index)}
            type="button"
          >
            {shortFilename(file.path)}
            <DiffStatusBadge status={file.status} />
          </button>
        ))}
      </div>

      {/* File detail */}
      {selectedFile ? (
        <div className="flex min-h-0 flex-1 flex-col">
          <div className="flex items-center justify-between border-border border-b px-3 py-1.5">
            <div className="flex min-w-0 items-center gap-2 font-mono text-xs">
              <FileText className="size-3.5 shrink-0 text-muted-foreground" />
              <span className="truncate">{selectedFile.path}</span>
              <span className="text-success">+{selectedFile.addedLines}</span>
              <span className="text-destructive">-{selectedFile.removedLines}</span>
            </div>
            <div className="flex gap-1">
              {selectedFile.fullPatchRef ? (
                <FetchPatchPageButton
                  conversationId={conversationId}
                  fullPatchRef={selectedFile.fullPatchRef}
                  onPageFetched={(page) => {
                    setPatchPagesByRef((current) => ({
                      ...current,
                      [selectedFile.fullPatchRef as string]: page,
                    }))
                  }}
                />
              ) : null}
              <CopyButton
                label={
                  selectedFile.fullPatchRef
                    ? t('diff.copyFullPatch', 'Copy full patch')
                    : t('diff.copyPreview', 'Copy diff preview')
                }
                disabled={copyingPatchRef === selectedFile.fullPatchRef}
                onClick={() => void copySelectedFile()}
              />
            </div>
          </div>
          {copyError ? (
            <div className="border-border border-b px-3 py-1 text-destructive text-xs">
              {t('diff.copyFailed', 'Copy failed')}
            </div>
          ) : null}

          {/* Risk flags */}
          {selectedFile.riskFlags && selectedFile.riskFlags.length > 0 ? (
            <div className="flex flex-wrap gap-1 border-border border-b px-3 py-1">
              {selectedFile.riskFlags.map((flag) => (
                <span
                  key={flag}
                  className="rounded bg-destructive/10 px-1.5 py-0.5 font-medium text-destructive text-xs"
                >
                  {flag}
                </span>
              ))}
            </div>
          ) : null}

          {/* Preview or placeholder */}
          <div className="min-h-0 flex-1 overflow-auto">
            {isBinaryOrGenerated ? (
              <div className="flex h-full items-center justify-center p-6 text-center text-muted-foreground text-sm">
                {t('diff.binaryOrGenerated')}
              </div>
            ) : selectedPatchPage ? (
              <pre className="bg-code-background p-3 font-mono text-[12px] leading-5">
                <code>{selectedPatchPage.patch}</code>
              </pre>
            ) : selectedFile.preview ? (
              <pre className="bg-code-background p-3 font-mono text-[12px] leading-5">
                <code>{selectedFile.preview}</code>
              </pre>
            ) : selectedFile.fullPatchRef ? (
              <div className="flex h-full items-center justify-center p-6 text-center text-muted-foreground text-sm">
                {t('diff.fetchPatchPageHint')}
              </div>
            ) : (
              <div className="flex h-full items-center justify-center p-6 text-center text-muted-foreground text-sm">
                {t('diff.noPreview')}
              </div>
            )}
          </div>
        </div>
      ) : (
        <div className="flex flex-1 items-center justify-center text-muted-foreground text-sm">
          {t('diff.noFiles')}
        </div>
      )}
    </div>
  )
}

function FetchPatchPageButton({
  conversationId,
  fullPatchRef,
  onPageFetched,
}: {
  conversationId: string
  fullPatchRef: string
  onPageFetched: (page: { patch: string; truncated: boolean }) => void
}) {
  const { t } = useTranslation('conversation')
  const commandClient = useCommandClient()
  const [fetching, setFetching] = useState(false)
  const [patchState, setPatchState] = useState<{
    loaded: boolean
    truncated: boolean
  } | null>(null)
  const [fetchError, setFetchError] = useState(false)

  const handleFetch = async () => {
    setFetching(true)
    setFetchError(false)
    try {
      const response = await commandClient.getConversationDiffPatch({
        conversationId,
        fullPatchRef,
      })
      onPageFetched({ patch: response.patch, truncated: response.truncated })
      setPatchState({
        loaded: Boolean(response.patch),
        truncated: response.truncated,
      })
    } catch {
      setPatchState(null)
      setFetchError(true)
    } finally {
      setFetching(false)
    }
  }

  if (patchState?.truncated) {
    return (
      <span className="px-2 text-muted-foreground text-xs">
        {t('diff.patchPageTruncated', 'Patch page truncated')}
      </span>
    )
  }
  if (patchState?.loaded) {
    return <span className="px-2 text-muted-foreground text-xs">{t('diff.patchPageLoaded')}</span>
  }

  return (
    <>
      <button
        className="rounded px-2 py-0.5 text-muted-foreground text-xs hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
        disabled={fetching}
        onClick={handleFetch}
        type="button"
      >
        {fetching ? t('diff.fetching') : t('diff.fetchPatchPage')}
      </button>
      {fetchError ? (
        <span className="px-2 text-destructive text-xs">
          {t('diff.fetchPatchPageFailed', 'Failed to load patch page')}
        </span>
      ) : null}
    </>
  )
}

function CopyButton({
  disabled = false,
  label,
  onClick,
}: {
  disabled?: boolean
  label: string
  onClick: () => void
}) {
  return (
    <button
      aria-label={label}
      className="inline-flex size-6 items-center justify-center rounded text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
      disabled={disabled}
      onClick={onClick}
      type="button"
    >
      <Copy className="size-3" />
    </button>
  )
}

function DiffStatusBadge({ status }: { status: ChangeSetFile['status'] }) {
  const colors: Record<string, string> = {
    added: 'bg-success/10 text-success',
    modified: 'bg-warning/10 text-warning',
    deleted: 'bg-destructive/10 text-destructive',
    renamed: 'bg-info/10 text-info',
  }
  return (
    <span className={cn('ml-1 rounded px-1 py-0 text-[10px]', colors[status] ?? 'bg-muted')}>
      {status}
    </span>
  )
}

function shortFilename(path: string) {
  return path.split('/').at(-1) ?? path
}
