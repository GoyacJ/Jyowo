import { useTranslation } from 'react-i18next'
import type { ProcessSegment, ProcessStep } from '@/shared/tauri/commands'
import { DiffPreview } from '../DiffPreview'
import { ArtifactImagePreview } from './artifact-segment-view'

export function ProcessPanel({
  conversationId,
  segment,
}: {
  conversationId: string
  segment: ProcessSegment
}) {
  const steps = [...(segment.steps ?? [])].sort((left, right) => left.order - right.order)

  return (
    <section className="rounded-md border border-border bg-surface px-3 py-2 text-sm">
      <div className="font-medium text-foreground">{segment.summary}</div>
      {steps.length > 0 ? (
        <ol className="mt-3 grid gap-3">
          {steps.map((step) => (
            <ProcessStepItem conversationId={conversationId} key={step.id} step={step} />
          ))}
        </ol>
      ) : null}
    </section>
  )
}

function ProcessStepItem({ conversationId, step }: { conversationId: string; step: ProcessStep }) {
  const { t } = useTranslation('conversation')
  const shouldCollapseBody = step.detail?.type === 'activity' && Boolean(step.body)

  return (
    <li className="grid gap-1.5 border-border border-l pl-3">
      <div className="font-medium text-foreground">{step.title}</div>
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
          {step.detail ? (
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
        <div className="overflow-hidden rounded-md border border-border bg-code-background">
          <div className="border-border border-b px-3 py-2 font-mono text-xs">{detail.command}</div>
          {detail.output ? (
            <pre className="max-h-48 overflow-auto px-3 py-2 text-xs leading-5">
              <code>{detail.output}</code>
            </pre>
          ) : null}
          {detail.exitCode !== undefined || detail.durationMs !== undefined ? (
            <div className="border-border border-t px-3 py-1.5 text-muted-foreground text-xs">
              {detail.exitCode !== undefined ? `exit ${detail.exitCode}` : null}
              {detail.exitCode !== undefined && detail.durationMs !== undefined ? ' · ' : null}
              {detail.durationMs !== undefined ? `${detail.durationMs} ms` : null}
            </div>
          ) : null}
        </div>
      )
    case 'diff':
      return (
        <div className="grid gap-2">
          {detail.files.map((file) => (
            <DiffPreview
              addedLineCount={file.addedLines}
              filename={file.path}
              key={file.path}
              lines={file.preview ? file.preview.split('\n') : []}
              maxVisibleLines={80}
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

function formatBytes(sizeBytes: number) {
  if (sizeBytes < 1024) {
    return `${sizeBytes} B`
  }
  if (sizeBytes < 1024 * 1024) {
    return `${(sizeBytes / 1024).toFixed(1)} KB`
  }
  return `${(sizeBytes / (1024 * 1024)).toFixed(1)} MB`
}
