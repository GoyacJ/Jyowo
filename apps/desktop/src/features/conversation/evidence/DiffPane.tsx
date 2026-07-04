import { Copy, FileText } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/shared/lib/utils'
import { useCommandClient } from '@/shared/tauri/react'
import type { ChangeSetFile } from '@/shared/tauri/commands'

export function DiffPane({
  conversationId,
  files,
  onChangeSetClick,
}: {
  conversationId: string
  files: ChangeSetFile[]
  onChangeSetClick?: () => void
}) {
  const { t } = useTranslation('conversation')
  const [selectedFileIndex, setSelectedFileIndex] = useState(0)
  const selectedFile = files[selectedFileIndex]
  const isBinaryOrGenerated =
    selectedFile?.riskFlags?.some((f) => f === 'binary' || f === 'generated') ?? false

  return (
    <div className="flex h-full flex-col">
      {/* File list */}
      <div className="flex flex-wrap gap-1 border-border border-b p-2">
        {files.map((file, index) => (
          <button
            className={cn(
              'rounded px-2 py-1 font-mono text-xs transition-colors hover:bg-muted',
              index === selectedFileIndex
                ? 'bg-muted text-foreground'
                : 'text-muted-foreground',
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
                <FetchFullPatchButton
                  conversationId={conversationId}
                  fullPatchRef={selectedFile.fullPatchRef}
                />
              ) : null}
              <CopyButton
                onClick={() => {
                  if (selectedFile.preview) {
                    void navigator.clipboard?.writeText(selectedFile.preview)
                  }
                }}
              />
            </div>
          </div>

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
            ) : selectedFile.preview ? (
              <pre className="bg-code-background p-3 font-mono text-[12px] leading-5">
                <code>{selectedFile.preview}</code>
              </pre>
            ) : selectedFile.fullPatchRef ? (
              <div className="flex h-full items-center justify-center p-6 text-center text-muted-foreground text-sm">
                {t('diff.fetchFullPatchHint')}
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

function FetchFullPatchButton({
  conversationId,
  fullPatchRef,
}: {
  conversationId: string
  fullPatchRef: string
}) {
  const { t } = useTranslation('conversation')
  const commandClient = useCommandClient()
  const [fetching, setFetching] = useState(false)
  const [patch, setPatch] = useState<string | null>(null)

  const handleFetch = async () => {
    setFetching(true)
    try {
      const response = await commandClient.getConversationDiffPatch({
        conversationId,
        fullPatchRef,
      })
      setPatch(response.patch)
      if (response.patch) {
        await navigator.clipboard?.writeText(response.patch)
      }
    } catch {
      setPatch(null)
    } finally {
      setFetching(false)
    }
  }

  if (patch) {
    return (
      <span className="px-2 text-muted-foreground text-xs">
        {t('diff.fullPatchCopied')}
      </span>
    )
  }

  return (
    <button
      className="rounded px-2 py-0.5 text-muted-foreground text-xs hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
      disabled={fetching}
      onClick={handleFetch}
      type="button"
    >
      {fetching ? t('diff.fetching') : t('diff.fetchFullPatch')}
    </button>
  )
}

function CopyButton({ onClick }: { onClick: () => void }) {
  return (
    <button
      aria-label="Copy"
      className="inline-flex size-6 items-center justify-center rounded text-muted-foreground hover:bg-muted hover:text-foreground"
      onClick={onClick}
      type="button"
    >
      <Copy className="size-3" />
    </button>
  )
}

function DiffStatusBadge({ status }: { status: ChangeSetFile['status'] }) {
  const colors: Record<string, string> = {
    added: 'bg-green-100 text-green-800',
    modified: 'bg-yellow-100 text-yellow-800',
    deleted: 'bg-red-100 text-red-800',
    renamed: 'bg-blue-100 text-blue-800',
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
