import { Copy, FileText } from 'lucide-react'

export type DiffViewerLine = {
  content: string
  lineNumber: number
  type: 'added' | 'context' | 'removed'
}

export interface DiffViewerProps {
  addedLineCount: number
  filename: string
  lines: DiffViewerLine[]
  maxVisibleLines?: number
  onCopy?: () => void
}

export function DiffViewer({
  addedLineCount,
  filename,
  lines,
  maxVisibleLines = 120,
  onCopy,
}: DiffViewerProps) {
  const visibleLines = lines.slice(0, maxVisibleLines)
  const hiddenLineCount = Math.max(0, lines.length - visibleLines.length)

  return (
    <section className="mt-3 overflow-hidden rounded-md border border-border bg-surface">
      <div className="flex items-center justify-between gap-3 border-border border-b px-4 py-1.5">
        <div className="flex min-w-0 items-center gap-2 font-mono text-xs">
          <FileText className="size-4 shrink-0 text-muted-foreground" />
          <span className="truncate">{filename}</span>
          <span className="shrink-0 text-success">+{addedLineCount}</span>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <button className="rounded-md border border-border px-3 py-1 text-xs" type="button">
            Open in editor
          </button>
          <button
            aria-label="Copy diff"
            className="rounded-md p-1.5 text-muted-foreground hover:bg-muted hover:text-foreground"
            type="button"
            onClick={onCopy}
          >
            <Copy className="size-4" />
          </button>
        </div>
      </div>
      <pre className="max-h-[360px] overflow-auto bg-code-background px-4 py-2.5 font-mono text-[12px] leading-5">
        {visibleLines.map((line) => (
          <code
            className={`block ${line.type === 'added' ? 'text-success' : ''} ${
              line.type === 'removed' ? 'text-destructive' : ''
            }`}
            key={`${line.lineNumber}-${line.content}`}
          >
            <span className="mr-3 text-muted-foreground">{line.lineNumber}</span>
            {getLinePrefix(line.type)}
            {line.content}
          </code>
        ))}
      </pre>
      {hiddenLineCount > 0 ? (
        <p className="border-border border-t px-4 py-1.5 text-muted-foreground text-xs">
          {hiddenLineCount} more lines hidden. Open in editor to inspect the full diff.
        </p>
      ) : null}
    </section>
  )
}

function getLinePrefix(type: DiffViewerLine['type']) {
  if (type === 'added') {
    return '+ '
  }

  if (type === 'removed') {
    return '- '
  }

  return '  '
}
