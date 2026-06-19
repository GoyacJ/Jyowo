import { DiffViewer, type DiffViewerLine } from './DiffViewer'

export interface DiffPreviewProps {
  filename: string
  addedLineCount: number
  lines: string[]
  maxVisibleLines?: number
}

export function DiffPreview({
  addedLineCount,
  filename,
  lines,
  maxVisibleLines,
}: DiffPreviewProps) {
  return (
    <DiffViewer
      addedLineCount={addedLineCount}
      filename={filename}
      lines={toLines(lines)}
      maxVisibleLines={maxVisibleLines}
    />
  )
}

function toLines(lines: string[]): DiffViewerLine[] {
  return lines.map((line, index) => ({
    content: line.replace(/^[+-]\s?/, ''),
    lineNumber: index + 1,
    type: line.startsWith('-') ? 'removed' : line.startsWith('+') ? 'added' : 'context',
  }))
}
