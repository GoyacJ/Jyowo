import type {
  AgentActivitySegment,
  ArtifactSegment,
  AssistantWork,
  ChangeSetFile,
  ClarificationRequestSegment,
  CommandExecution,
  ErrorSegment,
  NoticeSegment,
  ProcessStep,
  ReviewRequestSegment,
  TextSegment,
  ToolGroupSegment,
} from '@/shared/tauri/commands'

type FileEditRenderFile = {
  changeSetId: string
  path: string
  oldPath?: string
  status: ChangeSetFile['status']
  addedLines: number
  removedLines: number
  preview?: string
  fullPatchRef?: string
  riskFlags: ChangeSetFile['riskFlags']
}

type ActivityRenderItem = {
  id: string
  kind: 'file' | 'search' | 'tool' | 'command'
  label: string
  detail?: string
}

type CommandRenderItem = {
  id: string
  stepId: string
  status: ProcessStep['status']
  command: CommandExecution
}

export type TimelineRenderBlock =
  | { kind: 'assistantText'; id: string; order: number; segment: TextSegment }
  | {
      kind: 'fileEdit'
      id: string
      order: number
      processSegmentId: string
      steps: ProcessStep[]
      files: FileEditRenderFile[]
      defaultOpen: boolean
      forcedOpen: boolean
    }
  | {
      kind: 'activity'
      id: string
      order: number
      processSegmentId: string
      steps: ProcessStep[]
      title: string
      itemCount?: number
      items: ActivityRenderItem[]
      defaultOpen: boolean
      forcedOpen: boolean
    }
  | {
      kind: 'commandGroup'
      id: string
      order: number
      processSegmentId: string
      steps: ProcessStep[]
      commands: CommandRenderItem[]
      defaultOpen: boolean
      forcedOpen: boolean
    }
  | {
      kind: 'toolGroup'
      id: string
      order: number
      segment: ToolGroupSegment
      defaultOpen: boolean
      forcedOpen: boolean
    }
  | { kind: 'artifact'; id: string; order: number; segment: ArtifactSegment }
  | { kind: 'reviewRequest'; id: string; order: number; segment: ReviewRequestSegment }
  | {
      kind: 'clarificationRequest'
      id: string
      order: number
      segment: ClarificationRequestSegment
    }
  | { kind: 'notice'; id: string; order: number; segment: NoticeSegment }
  | { kind: 'error'; id: string; order: number; segment: ErrorSegment }
  | { kind: 'agentActivity'; id: string; order: number; segment: AgentActivitySegment }

type ProcessRenderGroupKind = 'activity' | 'commandGroup' | 'fileEdit'
type CommandStepDetail = Extract<NonNullable<ProcessStep['detail']>, { type: 'command' }>
type DiffStepDetail = Extract<NonNullable<ProcessStep['detail']>, { type: 'diff' }>
type ActivityStepDetail = Extract<NonNullable<ProcessStep['detail']>, { type: 'activity' }>

export function buildTimelineRenderBlocks(assistant: AssistantWork): TimelineRenderBlock[] {
  const blocks: TimelineRenderBlock[] = []

  for (const segment of sortByOrder(assistant.segments)) {
    switch (segment.kind) {
      case 'text':
        blocks.push({
          kind: 'assistantText',
          id: segment.id,
          order: segment.order,
          segment,
        })
        break
      case 'process':
        blocks.push(...buildProcessBlocks(segment.id, segment.order, segment.steps ?? []))
        break
      case 'toolGroup':
        blocks.push(toolGroupBlock(segment))
        break
      case 'artifact':
        blocks.push({ kind: 'artifact', id: segment.id, order: segment.order, segment })
        break
      case 'reviewRequest':
        blocks.push({ kind: 'reviewRequest', id: segment.id, order: segment.order, segment })
        break
      case 'clarificationRequest':
        blocks.push({ kind: 'clarificationRequest', id: segment.id, order: segment.order, segment })
        break
      case 'notice':
        blocks.push({ kind: 'notice', id: segment.id, order: segment.order, segment })
        break
      case 'error':
        blocks.push({ kind: 'error', id: segment.id, order: segment.order, segment })
        break
      case 'agentActivity':
        blocks.push({ kind: 'agentActivity', id: segment.id, order: segment.order, segment })
        break
      default:
        break
    }
  }

  return blocks
}

export function getDefaultRenderBlockDisclosure(block: TimelineRenderBlock): {
  defaultOpen: boolean
  forcedOpen: boolean
} {
  switch (block.kind) {
    case 'fileEdit':
    case 'activity':
    case 'commandGroup':
    case 'toolGroup':
      return {
        defaultOpen: block.defaultOpen,
        forcedOpen: block.forcedOpen,
      }
    default:
      return { defaultOpen: true, forcedOpen: false }
  }
}

function buildProcessBlocks(
  processSegmentId: string,
  processOrder: number,
  steps: ProcessStep[],
): TimelineRenderBlock[] {
  const blocks: TimelineRenderBlock[] = []
  let pendingGroup: { kind: ProcessRenderGroupKind; steps: ProcessStep[] } | undefined

  const flushPendingGroup = () => {
    if (!pendingGroup) {
      return
    }
    const block = buildProcessGroupBlock(processSegmentId, processOrder, pendingGroup)
    if (block) {
      blocks.push(block)
    }
    pendingGroup = undefined
  }

  for (const step of sortByOrder(steps)) {
    const groupKind = getProcessGroupKind(step)
    if (!groupKind) {
      flushPendingGroup()
      continue
    }
    if (!pendingGroup || pendingGroup.kind !== groupKind) {
      flushPendingGroup()
      pendingGroup = { kind: groupKind, steps: [step] }
      continue
    }
    pendingGroup.steps.push(step)
  }

  flushPendingGroup()
  return blocks
}

function buildProcessGroupBlock(
  processSegmentId: string,
  processOrder: number,
  group: { kind: ProcessRenderGroupKind; steps: ProcessStep[] },
): TimelineRenderBlock | undefined {
  const firstStep = group.steps[0]
  if (!firstStep) {
    return undefined
  }

  switch (group.kind) {
    case 'fileEdit': {
      const disclosure = disclosureForSteps(group.steps)
      return {
        kind: 'fileEdit',
        id: `process:${processSegmentId}:file-edit:${firstStep.id}`,
        order: processOrder,
        processSegmentId,
        steps: group.steps,
        files: fileEditFiles(group.steps),
        ...disclosure,
      }
    }
    case 'activity': {
      const disclosure = disclosureForSteps(group.steps)
      return {
        kind: 'activity',
        id: `process:${processSegmentId}:activity:${firstStep.id}`,
        order: processOrder,
        processSegmentId,
        steps: group.steps,
        title: activityTitle(firstStep),
        itemCount: activityItemCount(group.steps),
        items: activityItems(group.steps),
        ...disclosure,
      }
    }
    case 'commandGroup': {
      const commands = commandItems(group.steps)
      const forcedOpen = group.steps.some(isFailedOrRunningStep) || commands.some(isNonZeroCommand)
      return {
        kind: 'commandGroup',
        id: `process:${processSegmentId}:commands:${firstStep.id}`,
        order: processOrder,
        processSegmentId,
        steps: group.steps,
        commands,
        defaultOpen: forcedOpen,
        forcedOpen,
      }
    }
  }
}

function toolGroupBlock(segment: ToolGroupSegment): TimelineRenderBlock {
  const forcedOpen = segment.attempts.some((attempt) =>
    ['denied', 'failed', 'queued', 'running', 'waitingPermission'].includes(attempt.status),
  )
  return {
    kind: 'toolGroup',
    id: segment.id,
    order: segment.order,
    segment,
    defaultOpen: forcedOpen,
    forcedOpen,
  }
}

function getProcessGroupKind(step: ProcessStep): ProcessRenderGroupKind | undefined {
  if (step.kind === 'fileEdit' || step.kind === 'diff') {
    return 'fileEdit'
  }
  if (step.kind === 'fileRead' || step.kind === 'fileSearch') {
    return 'activity'
  }
  if (step.kind === 'command') {
    return 'commandGroup'
  }
  return undefined
}

function fileEditFiles(steps: ProcessStep[]): FileEditRenderFile[] {
  return steps.flatMap((step) => {
    const detail = diffDetail(step)
    if (!detail) {
      return []
    }
    return detail.files.map((file) => ({
      changeSetId: detail.id,
      path: file.path,
      oldPath: file.oldPath,
      status: file.status,
      addedLines: file.addedLines,
      removedLines: file.removedLines,
      preview: file.preview,
      fullPatchRef: file.fullPatchRef,
      riskFlags: file.riskFlags,
    }))
  })
}

function activityTitle(step: ProcessStep): string {
  const detail = activityDetail(step)
  return detail?.summary ?? step.title
}

function activityItemCount(steps: ProcessStep[]): number | undefined {
  let total = 0
  let hasCount = false
  for (const step of steps) {
    const count = activityDetail(step)?.itemCount
    if (count === undefined) {
      continue
    }
    total += count
    hasCount = true
  }
  return hasCount ? total : undefined
}

function activityItems(steps: ProcessStep[]): ActivityRenderItem[] {
  return steps.flatMap((step) => {
    const items = activityDetail(step)?.items ?? []
    return items.map((item) => ({
      id: activityItemId(step.id, item.kind, item.label, item.detail),
      kind: item.kind,
      label: item.label,
      detail: item.detail,
    }))
  })
}

function activityItemId(
  stepId: string,
  kind: ActivityRenderItem['kind'],
  label: string,
  detail: string | undefined,
) {
  return `${stepId}:${kind}:${encodeURIComponent(label)}:${encodeURIComponent(detail ?? '')}`
}

function commandItems(steps: ProcessStep[]): CommandRenderItem[] {
  return steps.flatMap((step) => {
    const detail = commandDetail(step)
    if (!detail) {
      return []
    }
    return [
      {
        id: step.id,
        stepId: step.id,
        status: step.status,
        command: {
          command: detail.command,
          cwd: detail.cwd,
          shell: detail.shell,
          sandbox: detail.sandbox,
          approvalRequestId: detail.approvalRequestId,
          exitCode: detail.exitCode,
          durationMs: detail.durationMs,
          stdoutPreview: detail.stdoutPreview,
          stderrPreview: detail.stderrPreview,
          fullOutputRef: detail.fullOutputRef,
          truncated: detail.truncated,
          redactionState: detail.redactionState,
          riskLevel: detail.riskLevel,
        },
      },
    ]
  })
}

function disclosureForSteps(steps: ProcessStep[]) {
  const forcedOpen = steps.some(isFailedOrRunningStep)
  return {
    defaultOpen: forcedOpen,
    forcedOpen,
  }
}

function isFailedOrRunningStep(step: ProcessStep) {
  return step.status === 'failed' || step.status === 'running'
}

function isNonZeroCommand(command: CommandRenderItem) {
  const exitCode = command.command.exitCode
  return exitCode !== undefined && exitCode !== 0
}

function activityDetail(step: ProcessStep): ActivityStepDetail | undefined {
  return step.detail?.type === 'activity' ? step.detail : undefined
}

function commandDetail(step: ProcessStep): CommandStepDetail | undefined {
  return step.detail?.type === 'command' ? step.detail : undefined
}

function diffDetail(step: ProcessStep): DiffStepDetail | undefined {
  return step.detail?.type === 'diff' ? step.detail : undefined
}

function sortByOrder<T extends { order: number }>(items: T[]): T[] {
  return [...items].sort((left, right) => left.order - right.order)
}
