import { FileDiff } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/shared/ui/button'

export function DiffPanel({
  error,
  loading,
  missing,
  onRetry,
  text,
}: {
  error?: boolean
  loading: boolean
  missing: boolean
  onRetry?: () => void
  text: string | null
}) {
  const { t } = useTranslation('tasks')
  if (text !== null && !loading && !error && !missing) {
    return <UnifiedDiff text={text} />
  }
  return (
    <ArtifactText
      empty={t('workbench.empty.changes')}
      error={error}
      loading={loading}
      missing={missing}
      onRetry={onRetry}
      text={text}
    />
  )
}

type DiffLineKind = 'addition' | 'context' | 'deletion' | 'hunk' | 'meta'

type ParsedDiffLine = {
  kind: DiffLineKind
  newLine: number | null
  oldLine: number | null
  text: string
}

type ParsedDiffFile = {
  label: string
  lines: ParsedDiffLine[]
}

export function parseUnifiedDiff(text: string): ParsedDiffFile[] {
  const files: ParsedDiffFile[] = []
  let current: ParsedDiffFile | null = null
  let oldLine: number | null = null
  let newLine: number | null = null

  const ensureFile = () => {
    if (!current) {
      current = { label: '', lines: [] }
      files.push(current)
    }
    return current
  }

  const sourceLines = text.replaceAll('\r\n', '\n').split('\n')
  if (sourceLines.at(-1) === '') sourceLines.pop()

  for (const line of sourceLines) {
    if (line.startsWith('diff --git ')) {
      const paths = line.match(/^diff --git (.+) (.+)$/)
      current = { label: cleanDiffPath(paths?.[2] ?? ''), lines: [] }
      files.push(current)
      current.lines.push({ kind: 'meta', newLine: null, oldLine: null, text: line })
      oldLine = null
      newLine = null
      continue
    }

    const file = ensureFile()
    if (line.startsWith('+++ ')) {
      const nextLabel = cleanDiffPath(line.slice(4))
      if (nextLabel && nextLabel !== '/dev/null') file.label = nextLabel
      file.lines.push({ kind: 'meta', newLine: null, oldLine: null, text: line })
      continue
    }
    if (line.startsWith('--- ')) {
      const previousLabel = cleanDiffPath(line.slice(4))
      if (!file.label && previousLabel !== '/dev/null') file.label = previousLabel
      file.lines.push({ kind: 'meta', newLine: null, oldLine: null, text: line })
      continue
    }

    const hunk = line.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/)
    if (hunk) {
      oldLine = Number(hunk[1])
      newLine = Number(hunk[2])
      file.lines.push({ kind: 'hunk', newLine: null, oldLine: null, text: line })
      continue
    }

    if (line.startsWith('+')) {
      file.lines.push({ kind: 'addition', newLine, oldLine: null, text: line })
      if (newLine !== null) newLine += 1
      continue
    }
    if (line.startsWith('-')) {
      file.lines.push({ kind: 'deletion', newLine: null, oldLine, text: line })
      if (oldLine !== null) oldLine += 1
      continue
    }
    if (line.startsWith(' ') && oldLine !== null && newLine !== null) {
      file.lines.push({ kind: 'context', newLine, oldLine, text: line })
      oldLine += 1
      newLine += 1
      continue
    }
    file.lines.push({ kind: 'meta', newLine: null, oldLine: null, text: line })
  }

  return files
}

function UnifiedDiff({ text }: { text: string }) {
  const { t } = useTranslation('tasks')
  const files = parseUnifiedDiff(text)
  return (
    <div className="min-h-full bg-background font-mono text-xs" data-testid="unified-diff">
      {files.map((file, fileIndex) => {
        const label = file.label || t('workbench.diff.changes')
        return (
          <section
            aria-label={t('workbench.diff.file', { file: label })}
            className={fileIndex > 0 ? 'border-border border-t' : undefined}
            key={`${label}:${fileIndex}`}
          >
            <header className="sticky top-0 z-10 flex min-h-9 items-center gap-2 border-border border-b bg-surface-raised/95 px-3 font-sans font-medium text-xs backdrop-blur-sm">
              <FileDiff aria-hidden="true" className="size-3.5 text-muted-foreground" />
              <span className="truncate" title={label}>
                {label}
              </span>
            </header>
            <div className="overflow-x-auto">
              <table className="w-full min-w-max border-collapse" aria-label={label}>
                <thead className="sr-only">
                  <tr>
                    <th>{t('workbench.diff.oldLineHeading')}</th>
                    <th>{t('workbench.diff.newLineHeading')}</th>
                    <th>{t('workbench.diff.contentHeading')}</th>
                  </tr>
                </thead>
                <tbody>
                  {file.lines.map((line, lineIndex) => (
                    <DiffRow key={`${lineIndex}:${line.text}`} line={line} />
                  ))}
                </tbody>
              </table>
            </div>
          </section>
        )
      })}
    </div>
  )
}

function DiffRow({ line }: { line: ParsedDiffLine }) {
  const { t } = useTranslation('tasks')
  if (line.kind === 'hunk' || line.kind === 'meta') {
    return (
      <tr
        className={
          line.kind === 'hunk'
            ? 'border-border/70 border-y bg-accent/35 text-accent-foreground'
            : 'text-muted-foreground'
        }
        data-diff-line={line.kind}
      >
        <td className="px-3 py-1.5 whitespace-pre" colSpan={3}>
          {line.kind === 'hunk' ? (
            <span className="sr-only">{t('workbench.diff.hunk')}: </span>
          ) : null}
          {line.text || ' '}
        </td>
      </tr>
    )
  }

  const labelKey =
    line.kind === 'addition'
      ? 'workbench.diff.added'
      : line.kind === 'deletion'
        ? 'workbench.diff.removed'
        : 'workbench.diff.unchanged'
  return (
    <tr
      className={
        line.kind === 'addition'
          ? 'bg-state-completed/10'
          : line.kind === 'deletion'
            ? 'bg-state-failed/10'
            : undefined
      }
      data-diff-line={line.kind}
    >
      <LineNumber kind="old" value={line.oldLine} />
      <LineNumber kind="new" value={line.newLine} />
      <td className="min-w-full py-0.5 pr-4 pl-3 whitespace-pre text-foreground/90">
        <span className="sr-only">{t(labelKey)}: </span>
        {line.text || ' '}
      </td>
    </tr>
  )
}

function LineNumber({ kind, value }: { kind: 'new' | 'old'; value: number | null }) {
  const { t } = useTranslation('tasks')
  return (
    <td
      aria-label={value === null ? undefined : t(`workbench.diff.${kind}Line`, { line: value })}
      className="w-12 select-none border-border/60 border-r px-2 py-0.5 text-right text-muted-foreground tabular-nums"
    >
      {value ?? ''}
    </td>
  )
}

function cleanDiffPath(value: string) {
  const path = value.trim().split('\t')[0] ?? ''
  return path.replace(/^['"]?[ab]\//, '').replace(/['"]$/, '')
}

export function ArtifactText({
  empty,
  error = false,
  loading,
  missing,
  onRetry,
  text,
}: {
  empty: string
  error?: boolean
  loading: boolean
  missing: boolean
  onRetry?: () => void
  text: string | null
}) {
  const { t } = useTranslation('tasks')
  if (loading) return <PanelSkeleton label={t('workbench.artifact.loading')} />
  if (error) {
    return (
      <PanelState>
        <span>{t('workbench.artifact.loadFailed')}</span>
        {onRetry ? (
          <Button onClick={onRetry} size="sm" type="button" variant="outline">
            {t('workbench.artifact.retry')}
          </Button>
        ) : null}
      </PanelState>
    )
  }
  if (missing) return <PanelState>{t('workbench.artifact.unavailable')}</PanelState>
  if (text === null) return <PanelState>{empty}</PanelState>
  return (
    <pre className="min-h-full overflow-auto whitespace-pre-wrap p-4 font-mono text-xs leading-6">
      {text}
    </pre>
  )
}

function PanelState({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex min-h-48 flex-col items-center justify-center gap-3 px-6 text-center text-muted-foreground text-sm">
      {children}
    </div>
  )
}

function PanelSkeleton({ label }: { label: string }) {
  return (
    <div aria-label={label} className="space-y-3 p-4" role="status">
      <span className="sr-only">{label}</span>
      <div className="h-3 w-2/3 animate-pulse rounded bg-muted" />
      <div className="h-3 w-full animate-pulse rounded bg-muted" />
      <div className="h-3 w-5/6 animate-pulse rounded bg-muted" />
      <div className="h-3 w-1/2 animate-pulse rounded bg-muted" />
    </div>
  )
}
