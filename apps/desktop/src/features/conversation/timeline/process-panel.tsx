import { CircleDot, Terminal } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useUiStore } from '@/shared/state/ui-store'
import type { ProcessSegment, ProcessStep } from '@/shared/tauri/commands'
import { ProcessStatusRow } from './process-status-row'
import { ProcessStepRow } from './process-step-row'

type ProcessDisplayItem =
  | { kind: 'step'; step: ProcessStep }
  | {
      defaultOpen: boolean
      id: string
      kind: 'group'
      steps: ProcessStep[]
      titleKey: 'timeline.processGroup.commandHistory' | 'timeline.processGroup.history'
    }

export function ProcessPanel({
  artifactRevisionIdsByArtifactId = {},
  conversationId,
  runId,
  segment,
}: {
  artifactRevisionIdsByArtifactId?: Record<string, string>
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
      step.detail?.type === 'command' &&
      (step.status === 'failed' || step.status === 'running' || isNonZeroCommand(step)),
  )
  const displayItems = buildProcessDisplayItems(steps, {
    hasActiveOrFailedCommand,
    latestCommandStepId,
  })

  return (
    <section className="grid gap-2 text-sm">
      <div className="text-foreground">{segment.summary}</div>
      {displayItems.length > 0 ? (
        <ol className="grid gap-3">
          {displayItems.map((item) =>
            item.kind === 'step' ? (
              <ProcessStepRow
                artifactRevisionIdsByArtifactId={artifactRevisionIdsByArtifactId}
                conversationId={conversationId}
                defaultDetailOpen={defaultDetailOpen(item.step, {
                  hasActiveOrFailedCommand,
                  latestCommandStepId,
                })}
                disclosureId={`process-step:${conversationId}:${runId}:${segment.id}:${item.step.id}`}
                key={item.step.id}
                step={item.step}
              />
            ) : (
              <ProcessStepGroup
                artifactRevisionIdsByArtifactId={artifactRevisionIdsByArtifactId}
                conversationId={conversationId}
                group={item}
                key={item.id}
                runId={runId}
                segmentId={segment.id}
              />
            ),
          )}
        </ol>
      ) : null}
    </section>
  )
}

function ProcessStepGroup({
  artifactRevisionIdsByArtifactId,
  conversationId,
  group,
  runId,
  segmentId,
}: {
  artifactRevisionIdsByArtifactId: Record<string, string>
  conversationId: string
  group: Extract<ProcessDisplayItem, { kind: 'group' }>
  runId: string
  segmentId: string
}) {
  const { t } = useTranslation('conversation')
  const disclosureId = `process-step-group:${conversationId}:${runId}:${segmentId}:${group.id}`
  const storedOpen = useUiStore((state) => state.evidenceDisclosureOpen[disclosureId])
  const setDisclosureOpen = useUiStore((state) => state.setEvidenceDisclosureOpen)
  const open = storedOpen ?? group.defaultOpen
  const Icon = group.steps.every((step) => step.detail?.type === 'command') ? Terminal : CircleDot

  return (
    <li className="grid gap-2">
      <ProcessStatusRow
        collapsible
        icon={Icon}
        onToggle={() => setDisclosureOpen(disclosureId, !open)}
        open={open}
        status="complete"
        title={t(group.titleKey, { count: group.steps.length })}
      />
      {open ? (
        <ol className="grid gap-3 pl-5">
          {group.steps.map((step) => (
            <ProcessStepRow
              artifactRevisionIdsByArtifactId={artifactRevisionIdsByArtifactId}
              conversationId={conversationId}
              defaultDetailOpen
              disclosureId={`process-step:${conversationId}:${runId}:${segmentId}:${group.id}:${step.id}`}
              key={step.id}
              step={step}
            />
          ))}
        </ol>
      ) : null}
    </li>
  )
}

function buildProcessDisplayItems(
  steps: ProcessStep[],
  context: {
    hasActiveOrFailedCommand: boolean
    latestCommandStepId?: string
  },
): ProcessDisplayItem[] {
  const items: ProcessDisplayItem[] = []
  let pendingGroup: ProcessStep[] = []

  const flushGroup = () => {
    if (pendingGroup.length === 0) {
      return
    }

    items.push({
      defaultOpen: false,
      id: `${pendingGroup[0]?.id ?? 'history'}:${pendingGroup.at(-1)?.id ?? 'history'}`,
      kind: 'group',
      steps: pendingGroup,
      titleKey: pendingGroup.every((step) => step.detail?.type === 'command')
        ? 'timeline.processGroup.commandHistory'
        : 'timeline.processGroup.history',
    })
    pendingGroup = []
  }

  for (const step of steps) {
    if (isLowSignalCompletedStep(step, context)) {
      pendingGroup.push(step)
      continue
    }

    flushGroup()
    items.push({ kind: 'step', step })
  }

  flushGroup()
  return items
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

  if (isNonZeroCommand(step)) {
    return true
  }

  if (step.detail?.type === 'command') {
    return step.id === latestCommandStepId && !hasActiveOrFailedCommand
  }

  return true
}

function isLowSignalCompletedStep(
  step: ProcessStep,
  {
    hasActiveOrFailedCommand,
    latestCommandStepId,
  }: {
    hasActiveOrFailedCommand: boolean
    latestCommandStepId?: string
  },
) {
  if (step.status !== 'complete' || isNonZeroCommand(step)) {
    return false
  }

  if (step.kind === 'fileRead' || step.kind === 'fileSearch') {
    return true
  }

  if (step.detail?.type !== 'command') {
    return false
  }

  return step.id !== latestCommandStepId || hasActiveOrFailedCommand
}

function isNonZeroCommand(step: ProcessStep) {
  return (
    step.detail?.type === 'command' &&
    step.detail.exitCode !== undefined &&
    step.detail.exitCode !== 0
  )
}
