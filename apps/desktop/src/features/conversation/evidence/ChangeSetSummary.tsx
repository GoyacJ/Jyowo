import { FilePenLine, AlertTriangle } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { ChangeSet, ChangeSetFile } from '@/shared/tauri/commands'

export function ChangeSetSummary({
  changeSet,
  onClick,
}: {
  changeSet: ChangeSet
  onClick?: () => void
}) {
  const { t } = useTranslation('conversation')
  const totals = computeTotals(changeSet.files)
  const hasRiskFlags = changeSet.files.some((f) => f.riskFlags && f.riskFlags.length > 0)

  return (
    <button
      className="flex w-full items-center gap-3 rounded-md border border-border px-3 py-2 text-left transition-colors hover:bg-muted/50 focus-visible:ring-2 focus-visible:ring-ring"
      data-changeset-id={changeSet.id}
      onClick={onClick}
      type="button"
    >
      <FilePenLine className="size-4 shrink-0 text-muted-foreground" />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate font-medium text-sm">{changeSet.summary}</span>
          <span className="shrink-0 font-mono text-success text-xs">
            +{totals.added}
          </span>
          <span className="shrink-0 font-mono text-destructive text-xs">
            -{totals.removed}
          </span>
          {hasRiskFlags ? (
            <AlertTriangle className="size-3 shrink-0 text-yellow-500" />
          ) : null}
        </div>
        <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-muted-foreground text-xs">
          <span>
            {t('diff.fileCount', { count: changeSet.files.length })}
          </span>
          {totals.addedCount > 0 ? (
            <span>{t('diff.filesAdded', { count: totals.addedCount })}</span>
          ) : null}
          {totals.modifiedCount > 0 ? (
            <span>{t('diff.filesModified', { count: totals.modifiedCount })}</span>
          ) : null}
          {totals.deletedCount > 0 ? (
            <span className="text-destructive">
              {t('diff.filesDeleted', { count: totals.deletedCount })}
            </span>
          ) : null}
        </div>
      </div>
    </button>
  )
}

function computeTotals(files: ChangeSetFile[]) {
  let added = 0
  let removed = 0
  let addedCount = 0
  let modifiedCount = 0
  let deletedCount = 0

  for (const file of files) {
    added += file.addedLines
    removed += file.removedLines
    switch (file.status) {
      case 'added':
        addedCount++
        break
      case 'modified':
        modifiedCount++
        break
      case 'deleted':
        deletedCount++
        break
      case 'renamed':
        modifiedCount++
        break
    }
  }

  return { added, removed, addedCount, modifiedCount, deletedCount }
}
