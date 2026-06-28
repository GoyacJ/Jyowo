import { Copy, FileText } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { BundledLanguage, BundledTheme } from 'shiki'
import { highlightCode } from '@/shared/code/highlight'
import { cn } from '@/shared/lib/utils'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/shared/ui/tooltip'

export type DiffEvidenceLine = {
  content: string
  newLineNumber?: number
  oldLineNumber?: number
  prefix: '+' | '-' | ' '
  type: 'added' | 'removed' | 'context'
}

export type DiffEvidenceBlockProps = {
  addedLineCount: number
  filename: string
  lines: DiffEvidenceLine[]
  maxVisibleLines?: number
  onCopy?: () => void
  removedLineCount: number
}

export function DiffEvidenceBlock({
  addedLineCount,
  filename,
  lines,
  maxVisibleLines = 120,
  onCopy,
  removedLineCount,
}: DiffEvidenceBlockProps) {
  const { t } = useTranslation('conversation')
  const visibleLines = lines.slice(0, maxVisibleLines)
  const hiddenLineCount = Math.max(0, lines.length - visibleLines.length)
  const copyText = useMemo(() => visibleLines.map(formatDiffLineForCopy).join('\n'), [visibleLines])
  const highlightedLines = useHighlightedDiffLines(filename, visibleLines)

  return (
    <section className="overflow-hidden rounded-md border border-border bg-surface">
      <div className="flex h-8 items-center justify-between gap-3 border-border border-b px-3">
        <div className="flex min-w-0 items-center gap-2 font-mono text-xs">
          <FileText className="size-3.5 shrink-0 text-muted-foreground" />
          <span className="truncate">{filename}</span>
          <span className="shrink-0 text-success">+{addedLineCount}</span>
          <span className="shrink-0 text-destructive">-{removedLineCount}</span>
        </div>
        <TooltipProvider delayDuration={150}>
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                aria-label={t('diff.copy')}
                className="inline-flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                type="button"
                onClick={() => {
                  if (onCopy) {
                    onCopy()
                    return
                  }
                  void navigator.clipboard?.writeText(copyText)
                }}
              >
                <Copy className="size-3.5" />
              </button>
            </TooltipTrigger>
            <TooltipContent>{t('diff.copy')}</TooltipContent>
          </Tooltip>
        </TooltipProvider>
      </div>
      <div
        className="max-h-[360px] overflow-auto bg-code-background font-mono text-[12px] leading-5"
        data-testid="diff-scroll-region"
      >
        {visibleLines.map((line, index) => {
          const highlightedLine = highlightedLines?.[index]

          return (
            <div
              className={cn(
                'grid min-w-max grid-cols-[44px_20px_minmax(0,1fr)]',
                line.type === 'added' ? 'bg-success/10 text-success' : null,
                line.type === 'removed' ? 'bg-destructive/10 text-destructive' : null,
              )}
              key={`${index}:${line.prefix}:${line.content}`}
            >
              <span className="select-none border-border border-r px-2 text-right text-muted-foreground tabular-nums">
                {formatLineNumber(line)}
              </span>
              <span className="select-none text-center text-muted-foreground">{line.prefix}</span>
              <span className="pr-4 whitespace-pre">
                {highlightedLine ? (
                  <span
                    // Shiki escapes source code before returning syntax spans.
                    dangerouslySetInnerHTML={{ __html: highlightedLine }}
                  />
                ) : (
                  line.content
                )}
              </span>
            </div>
          )
        })}
      </div>
      {hiddenLineCount > 0 ? (
        <p className="border-border border-t px-3 py-1.5 text-muted-foreground text-xs">
          {t('diff.hiddenLines', { count: hiddenLineCount })}
        </p>
      ) : null}
    </section>
  )
}

function useHighlightedDiffLines(filename: string, lines: DiffEvidenceLine[]) {
  const [highlightedLines, setHighlightedLines] = useState<string[] | null>(null)
  const code = useMemo(() => lines.map((line) => line.content).join('\n'), [lines])

  useEffect(() => {
    const language = languageFromFilename(filename)
    if (!language || code.trim().length === 0) {
      setHighlightedLines(null)
      return
    }

    let cancelled = false
    const theme = currentShikiTheme()

    highlightCode(code, { lang: language, theme })
      .then((html) => {
        if (!cancelled) {
          setHighlightedLines(extractShikiLineHtml(html))
        }
      })
      .catch(() => {
        if (!cancelled) {
          setHighlightedLines(null)
        }
      })

    return () => {
      cancelled = true
    }
  }, [code, filename])

  return highlightedLines
}

function languageFromFilename(filename: string): BundledLanguage | null {
  const extension = filename.split('.').at(-1)?.toLowerCase()
  const languageByExtension: Partial<Record<string, BundledLanguage>> = {
    css: 'css',
    js: 'javascript',
    json: 'json',
    jsx: 'jsx',
    md: 'markdown',
    mjs: 'javascript',
    rs: 'rust',
    ts: 'typescript',
    tsx: 'tsx',
  }

  return extension ? (languageByExtension[extension] ?? null) : null
}

function currentShikiTheme(): BundledTheme {
  if (typeof document === 'undefined') {
    return 'github-light'
  }

  return document.documentElement.classList.contains('dark') ||
    document.querySelector('.dark') !== null
    ? 'github-dark'
    : 'github-light'
}

function extractShikiLineHtml(html: string) {
  if (typeof DOMParser === 'undefined') {
    return null
  }

  const document = new DOMParser().parseFromString(html, 'text/html')
  const lineElements = Array.from(document.querySelectorAll('.line'))
  return lineElements.length > 0 ? lineElements.map((line) => line.innerHTML) : null
}

export function parseDiffEvidenceLines(preview: string): DiffEvidenceLine[] {
  const rawLines = preview.split('\n')
  let oldLineNumber: number | undefined
  let newLineNumber: number | undefined
  let hasHunk = false

  return rawLines.map((rawLine): DiffEvidenceLine => {
    const hunk = parseHunkHeader(rawLine)
    if (hunk) {
      oldLineNumber = hunk.oldStart
      newLineNumber = hunk.newStart
      hasHunk = true
      return {
        content: rawLine,
        prefix: ' ',
        type: 'context',
      }
    }

    if (rawLine.startsWith('+++') || rawLine.startsWith('---')) {
      return {
        content: rawLine,
        prefix: ' ',
        type: 'context',
      }
    }

    if (rawLine.startsWith('+')) {
      const line = {
        content: rawLine.slice(1),
        newLineNumber: hasHunk ? newLineNumber : undefined,
        prefix: '+' as const,
        type: 'added' as const,
      }
      if (newLineNumber !== undefined) {
        newLineNumber += 1
      }
      return line
    }

    if (rawLine.startsWith('-')) {
      const line = {
        content: rawLine.slice(1),
        oldLineNumber: hasHunk ? oldLineNumber : undefined,
        prefix: '-' as const,
        type: 'removed' as const,
      }
      if (oldLineNumber !== undefined) {
        oldLineNumber += 1
      }
      return line
    }

    const content = rawLine.startsWith(' ') ? rawLine.slice(1) : rawLine
    const line = {
      content,
      newLineNumber: hasHunk ? newLineNumber : undefined,
      oldLineNumber: hasHunk ? oldLineNumber : undefined,
      prefix: ' ' as const,
      type: 'context' as const,
    }
    if (oldLineNumber !== undefined) {
      oldLineNumber += 1
    }
    if (newLineNumber !== undefined) {
      newLineNumber += 1
    }
    return line
  })
}

function parseHunkHeader(line: string) {
  const match = /^@@ -(?<oldStart>\d+)(?:,\d+)? \+(?<newStart>\d+)(?:,\d+)? @@/.exec(line)
  if (!match?.groups) {
    return null
  }

  return {
    oldStart: Number(match.groups.oldStart),
    newStart: Number(match.groups.newStart),
  }
}

function formatLineNumber(line: DiffEvidenceLine) {
  if (line.oldLineNumber !== undefined && line.newLineNumber !== undefined) {
    return `${line.oldLineNumber}/${line.newLineNumber}`
  }
  if (line.oldLineNumber !== undefined) {
    return String(line.oldLineNumber)
  }
  if (line.newLineNumber !== undefined) {
    return String(line.newLineNumber)
  }
  return ''
}

function formatDiffLineForCopy(line: DiffEvidenceLine) {
  return `${line.prefix}${line.content}`
}
