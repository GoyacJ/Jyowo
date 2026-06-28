import {
  CircleDot,
  FilePenLine,
  FileText,
  Image,
  type LucideIcon,
  Search,
  Terminal,
  Wrench,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useUiStore } from '@/shared/state/ui-store'
import type { ProcessStep } from '@/shared/tauri/commands'
import { ArtifactImagePreview } from './artifact-segment-view'
import { CommandEvidenceBlock } from './command-evidence-block'
import { DiffEvidenceBlock, parseDiffEvidenceLines } from './diff-evidence-block'
import { ProcessStatusRow } from './process-status-row'

export function ProcessStepRow({
  conversationId,
  defaultDetailOpen = true,
  disclosureId,
  step,
}: {
  conversationId: string
  defaultDetailOpen?: boolean
  disclosureId: string
  step: ProcessStep
}) {
  const { t } = useTranslation('conversation')
  const detail = step.detail
  const storedOpen = useUiStore((state) => state.evidenceDisclosureOpen[disclosureId])
  const setDisclosureOpen = useUiStore((state) => state.setEvidenceDisclosureOpen)
  const shouldCollapseBody = detail?.type === 'activity' && Boolean(step.body)
  const forcedOpen = step.status === 'failed' || step.status === 'running'
  const canToggle = detail !== undefined && detail.type !== 'activity' && !forcedOpen
  const detailOpen = forcedOpen || (storedOpen ?? defaultDetailOpen)

  return (
    <li className="grid gap-1.5">
      <ProcessStatusRow
        collapsible={canToggle}
        countLabel={getCountLabel(step)}
        durationMs={
          detail?.type === 'command' || detail?.type === 'tool' ? detail.durationMs : undefined
        }
        icon={getStepIcon(step)}
        onToggle={canToggle ? () => setDisclosureOpen(disclosureId, !detailOpen) : undefined}
        open={detailOpen}
        status={step.status}
        title={step.title}
      />
      {step.status === 'withheld' ? (
        <p className="text-muted-foreground text-sm">{t('timeline.processWithheld')}</p>
      ) : (
        <>
          {shouldCollapseBody ? (
            <details>
              <summary className="cursor-pointer text-muted-foreground text-sm">
                {t('timeline.processStepDetails')}
              </summary>
              <p className="mt-1 whitespace-pre-wrap text-muted-foreground text-sm leading-6">
                {step.body}
              </p>
            </details>
          ) : step.body ? (
            <p className="whitespace-pre-wrap text-muted-foreground text-sm leading-6">
              {step.body}
            </p>
          ) : null}
          {detail && detailOpen ? (
            <ProcessStepDetailView conversationId={conversationId} step={step} />
          ) : null}
        </>
      )}
    </li>
  )
}

function ProcessStepDetailView({
  conversationId,
  step,
}: {
  conversationId: string
  step: ProcessStep
}) {
  const detail = step.detail
  if (!detail) {
    return null
  }

  switch (detail.type) {
    case 'activity':
      return (
        <p className="text-muted-foreground text-sm">
          {detail.summary}
          {detail.itemCount !== undefined ? ` · ${detail.itemCount}` : null}
        </p>
      )
    case 'command':
      return (
        <CommandEvidenceBlock
          command={detail.command}
          durationMs={detail.durationMs}
          exitCode={detail.exitCode}
          output={detail.output}
        />
      )
    case 'diff':
      return (
        <div className="grid gap-2">
          {detail.files.map((file) => (
            <DiffEvidenceBlock
              addedLineCount={file.addedLines}
              filename={shortFilename(file.path)}
              key={file.path}
              lines={file.preview ? parseDiffEvidenceLines(file.preview) : []}
              maxVisibleLines={80}
              removedLineCount={file.removedLines}
            />
          ))}
        </div>
      )
    case 'tool':
      return (
        <p className="text-muted-foreground text-sm">
          {detail.toolName}
          {detail.outputSummary ? ` · ${detail.outputSummary}` : null}
          {detail.durationMs !== undefined ? ` · ${detail.durationMs} ms` : null}
        </p>
      )
    case 'artifact':
      return (
        <div>
          <p className="text-muted-foreground text-sm">
            {detail.media.kind} · {detail.media.mimeType} · {formatBytes(detail.media.sizeBytes)}
          </p>
          {detail.media.kind === 'image' ? (
            <ArtifactImagePreview
              artifactId={detail.artifactId}
              conversationId={conversationId}
              title={step.title}
            />
          ) : null}
        </div>
      )
  }
}

function getStepIcon(step: ProcessStep): LucideIcon {
  if (step.status === 'failed') {
    return getKindIcon(step.kind)
  }

  return getKindIcon(step.kind)
}

function getKindIcon(kind: ProcessStep['kind']): LucideIcon {
  switch (kind) {
    case 'fileEdit':
    case 'diff':
      return FilePenLine
    case 'fileRead':
      return FileText
    case 'fileSearch':
      return Search
    case 'command':
      return Terminal
    case 'tool':
      return Wrench
    case 'artifact':
      return Image
    case 'activity':
    case 'reasoning':
    case 'synthesis':
    case 'withheld':
      return CircleDot
  }
}

function getCountLabel(step: ProcessStep) {
  if (step.detail?.type === 'activity' && step.detail.itemCount !== undefined) {
    return String(step.detail.itemCount)
  }

  return undefined
}

function shortFilename(path: string) {
  return path.split('/').at(-1) ?? path
}

function formatBytes(sizeBytes: number) {
  if (sizeBytes < 1024) {
    return `${sizeBytes} B`
  }
  if (sizeBytes < 1024 * 1024) {
    return `${(sizeBytes / 1024).toFixed(1)} KB`
  }
  return `${(sizeBytes / (1024 * 1024)).toFixed(1)} MB`
}
