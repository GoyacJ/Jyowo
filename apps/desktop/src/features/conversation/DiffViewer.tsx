import { DiffEvidenceBlock, type DiffEvidenceLine } from './timeline/diff-evidence-block'

export type DiffViewerLine = {
  content: string
  lineNumber: number
  type: 'added' | 'context' | 'removed'
}

export interface DiffViewerProps {
  addedLineCount: number
  filename: string
  lines: Array<DiffViewerLine | DiffEvidenceLine>
  maxVisibleLines?: number
  onCopy?: () => void
  removedLineCount?: number
}

export function DiffViewer({
  addedLineCount,
  filename,
  lines,
  maxVisibleLines = 120,
  onCopy,
  removedLineCount,
}: DiffViewerProps) {
  return (
    <DiffEvidenceBlock
      addedLineCount={addedLineCount}
      filename={filename}
      lines={toEvidenceLines(lines)}
      maxVisibleLines={maxVisibleLines}
      removedLineCount={removedLineCount ?? countRemovedLines(lines)}
      onCopy={onCopy}
    />
  )
}

function toEvidenceLines(lines: Array<DiffViewerLine | DiffEvidenceLine>): DiffEvidenceLine[] {
  return lines.map((line) => {
    if ('prefix' in line) {
      return line
    }

    return {
      content: line.content,
      newLineNumber: line.type === 'removed' ? undefined : line.lineNumber,
      oldLineNumber: line.type === 'added' ? undefined : line.lineNumber,
      prefix: line.type === 'added' ? '+' : line.type === 'removed' ? '-' : ' ',
      type: line.type,
    }
  })
}

function countRemovedLines(lines: Array<DiffViewerLine | DiffEvidenceLine>) {
  return lines.filter((line) => line.type === 'removed').length
}
