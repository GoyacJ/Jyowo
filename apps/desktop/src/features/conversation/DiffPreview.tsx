import { DiffViewer } from './DiffViewer'
import { parseDiffEvidenceLines } from './timeline/diff-evidence-block'

export interface DiffPreviewProps {
  filename: string
  addedLineCount: number
  lines: string[]
  maxVisibleLines?: number
  removedLineCount?: number
}

export function DiffPreview({
  addedLineCount,
  filename,
  lines,
  maxVisibleLines,
  removedLineCount,
}: DiffPreviewProps) {
  return (
    <DiffViewer
      addedLineCount={addedLineCount}
      filename={filename}
      lines={parseDiffEvidenceLines(lines.join('\n'))}
      maxVisibleLines={maxVisibleLines}
      removedLineCount={removedLineCount ?? countRemovedLines(lines)}
    />
  )
}

function countRemovedLines(lines: string[]) {
  return lines.filter((line) => line.startsWith('-') && !line.startsWith('---')).length
}
