import type { ProcessSegment, ProcessStep } from '@/shared/tauri/commands'
import { ProcessStepRow } from './process-step-row'

export function ProcessPanel({
  conversationId,
  runId,
  segment,
}: {
  conversationId: string
  runId: string
  segment: ProcessSegment
}) {
  const steps = [...(segment.steps ?? [])].sort((left, right) => left.order - right.order)
  const latestCommandStepId = [...steps]
    .reverse()
    .find((step) => step.detail?.type === 'command')?.id
  const hasActiveOrFailedCommand = steps.some(
    (step) =>
      step.detail?.type === 'command' && (step.status === 'failed' || step.status === 'running'),
  )

  return (
    <section className="grid gap-2 text-sm">
      <div className="text-foreground">{segment.summary}</div>
      {steps.length > 0 ? (
        <ol className="grid gap-3">
          {steps.map((step) => (
            <ProcessStepRow
              conversationId={conversationId}
              defaultDetailOpen={defaultDetailOpen(step, {
                hasActiveOrFailedCommand,
                latestCommandStepId,
              })}
              disclosureId={`process-step:${conversationId}:${runId}:${segment.id}:${step.id}`}
              key={step.id}
              step={step}
            />
          ))}
        </ol>
      ) : null}
    </section>
  )
}

function defaultDetailOpen(
  step: ProcessStep,
  {
    hasActiveOrFailedCommand,
    latestCommandStepId,
  }: {
    hasActiveOrFailedCommand: boolean
    latestCommandStepId?: string
  },
) {
  if (step.status === 'failed' || step.status === 'running') {
    return true
  }

  if (step.detail?.type === 'command') {
    return step.id === latestCommandStepId && !hasActiveOrFailedCommand
  }

  return true
}
