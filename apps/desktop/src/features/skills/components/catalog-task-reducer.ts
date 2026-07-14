import type {
  SkillCatalogEntry,
  SkillCatalogInstallProgressPayload,
  SkillCatalogInstallTask,
} from '@/shared/tauri/commands'

export type CatalogInstallTasksState = { tasks: SkillCatalogInstallTask[] }

export function catalogInstallTaskKey(input: {
  entryId: string
  sourceId: string
  version?: string | null
}) {
  return `${input.sourceId}\u0000${input.entryId}\u0000${input.version ?? ''}`
}

function parsedUpdatedAt(task: SkillCatalogInstallTask) {
  const timestamp = Date.parse(task.updatedAt)
  if (!Number.isFinite(timestamp)) {
    throw new Error(`Invalid catalog task updatedAt: ${task.updatedAt}`)
  }
  return timestamp
}

export function reduceCatalogInstallTask(
  current: CatalogInstallTasksState | undefined,
  nextTask: SkillCatalogInstallTask,
): CatalogInstallTasksState {
  const currentTasks = current?.tasks ?? []
  const nextTimestamp = parsedUpdatedAt(nextTask)
  const existing = currentTasks.find((task) => task.operationId === nextTask.operationId)

  if (existing && parsedUpdatedAt(existing) > nextTimestamp) {
    return { tasks: currentTasks }
  }

  return {
    tasks: [...currentTasks.filter((task) => task.operationId !== nextTask.operationId), nextTask],
  }
}

export function findCatalogInstallTask(
  tasks: SkillCatalogInstallTask[],
  entry: SkillCatalogEntry | null | undefined,
) {
  if (!entry) {
    return null
  }

  const entryKey = catalogInstallTaskKey(entry)
  return (
    tasks
      .filter((task) => catalogInstallTaskKey(task) === entryKey)
      .sort((left, right) => parsedUpdatedAt(right) - parsedUpdatedAt(left))[0] ?? null
  )
}

export function catalogInstallTaskFromProgress(
  progress: SkillCatalogInstallProgressPayload,
  existing: SkillCatalogInstallTask | null,
  updatedAt = new Date().toISOString(),
): SkillCatalogInstallTask {
  assertCatalogInstallProgressPayload(progress)
  const status =
    progress.stage === 'completed'
      ? 'completed'
      : progress.stage === 'failed'
        ? 'failed'
        : progress.stage === 'interrupted'
          ? 'interrupted'
          : 'running'

  if (existing && isTerminalCatalogInstallTask(existing)) {
    return existing
  }

  if (
    existing?.status === 'running' &&
    status === 'running' &&
    (progress.percent < existing.percent ||
      progressStageRank(progress.stage) < progressStageRank(existing.stage))
  ) {
    return existing
  }

  const task = {
    entryId: progress.entryId,
    message: progress.message,
    operationId: progress.operationId,
    percent: progress.percent,
    sourceId: progress.sourceId,
    stage: progress.stage,
    startedAt: existing?.startedAt ?? updatedAt,
    status,
    updatedAt,
    version: progress.version,
  } satisfies SkillCatalogInstallTask

  parsedUpdatedAt(task)
  return task
}

export function isTerminalCatalogInstallTask(task: SkillCatalogInstallTask) {
  return task.status !== 'running'
}

const progressStages = new Set([
  'preparing',
  'resolving',
  'checking',
  'downloading',
  'validating',
  'copying',
  'reloading',
  'completed',
  'failed',
  'interrupted',
])

const progressStageOrder = [
  'preparing',
  'resolving',
  'checking',
  'downloading',
  'validating',
  'copying',
  'reloading',
] as const

function progressStageRank(stage: SkillCatalogInstallProgressPayload['stage']) {
  const rank = progressStageOrder.indexOf(stage as (typeof progressStageOrder)[number])
  return rank === -1 ? progressStageOrder.length : rank
}

export function assertCatalogInstallProgressPayload(
  value: unknown,
): asserts value is SkillCatalogInstallProgressPayload {
  if (typeof value !== 'object' || value === null) {
    throw new Error('Invalid catalog install progress payload')
  }

  const payload = value as Record<string, unknown>
  if (
    typeof payload.entryId !== 'string' ||
    payload.entryId.length === 0 ||
    typeof payload.operationId !== 'string' ||
    payload.operationId.length === 0 ||
    typeof payload.sourceId !== 'string' ||
    payload.sourceId.length === 0 ||
    typeof payload.percent !== 'number' ||
    !Number.isInteger(payload.percent) ||
    payload.percent < 0 ||
    payload.percent > 100 ||
    typeof payload.stage !== 'string' ||
    !progressStages.has(payload.stage)
  ) {
    throw new Error('Invalid catalog install progress payload')
  }
}
