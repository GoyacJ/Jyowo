import { ExternalLink, FilePenLine } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useUiStore } from '@/shared/state/ui-store'
import { DiffEvidenceBlock, parseDiffEvidenceLines } from './diff-evidence-block'
import { EvidenceDisclosure } from './evidence-disclosure'
import { useTimelineBlockDisclosure } from './timeline-disclosure-state'
import type { TimelineRenderBlock } from './timeline-render-blocks'

type FileEditBlock = Extract<TimelineRenderBlock, { kind: 'fileEdit' }>

export function FileEditRenderBlock({
  block,
  conversationId,
  runId,
}: {
  block: FileEditBlock
  conversationId: string
  runId: string
}) {
  const { t } = useTranslation('conversation')
  const { open, setOpen } = useTimelineBlockDisclosure({ block, conversationId, runId })
  const setSelection = useUiStore((state) => state.setWorkbenchSelection)
  const setInspectorOpen = useUiStore((state) => state.setInspectorOpen)
  const firstChangeSetId = block.files[0]?.changeSetId
  const firstFileSummary = block.files[0] ? fileSummary(block.files[0]) : undefined

  return (
    <EvidenceDisclosure
      actions={
        firstChangeSetId ? (
          <button
            aria-label={t('timeline.renderBlocks.openDiff')}
            className="inline-flex size-7 items-center justify-center rounded text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
            onClick={() => {
              setSelection({
                kind: 'diff',
                conversationId,
                changeSetId: firstChangeSetId,
              })
              setInspectorOpen(true)
            }}
            type="button"
          >
            <ExternalLink className="size-3.5" />
          </button>
        ) : null
      }
      forcedOpen={block.forcedOpen}
      icon={FilePenLine}
      id={block.id}
      meta={firstFileSummary}
      onOpenChange={setOpen}
      open={open}
      title={t('timeline.renderBlocks.fileEditSummary', { count: block.files.length })}
    >
      <div className="grid min-w-0 gap-2">
        <p className="font-medium text-muted-foreground text-xs">
          {t('timeline.renderBlocks.editedFiles')}
        </p>
        <ul className="grid gap-1">
          {block.files.map((file) => (
            <li className="flex min-w-0 items-center gap-2 text-xs" key={fileKey(file)}>
              <span className="min-w-0 truncate">{shortFilename(file.path)}</span>
              <span className="shrink-0 text-success">+{file.addedLines}</span>
              <span className="shrink-0 text-destructive">-{file.removedLines}</span>
            </li>
          ))}
        </ul>
        <div className="grid gap-2">
          {block.files
            .filter((file) => file.preview)
            .map((file) => (
              <DiffEvidenceBlock
                addedLineCount={file.addedLines}
                filename={shortFilename(file.path)}
                key={`${file.changeSetId}:${file.path}:preview`}
                lines={parseDiffEvidenceLines(file.preview ?? '')}
                maxVisibleLines={80}
                removedLineCount={file.removedLines}
              />
            ))}
        </div>
      </div>
    </EvidenceDisclosure>
  )
}

function fileSummary(file: FileEditBlock['files'][number]) {
  return `${shortFilename(file.path)} +${file.addedLines} -${file.removedLines}`
}

function fileKey(file: FileEditBlock['files'][number]) {
  return `${file.changeSetId}:${file.path}:${file.oldPath ?? ''}`
}

function shortFilename(path: string) {
  return path.split('/').at(-1) ?? path
}
