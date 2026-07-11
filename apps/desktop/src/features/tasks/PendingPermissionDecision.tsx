import { ShieldAlert } from 'lucide-react'
import { useState } from 'react'

import type { PermissionProjection, TypedUlid } from '@/generated/daemon-protocol'
import { requireAcceptedCommand } from './task-command'
import type { TaskCommandExecutor } from './use-task-command-executor'

export function PendingPermissionDecision({
  executeCommand,
  permission,
  taskId,
}: {
  executeCommand: TaskCommandExecutor
  permission: PermissionProjection
  taskId: TypedUlid
}) {
  const [submittingLabel, setSubmittingLabel] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const details = permission.details

  if (!details) {
    return (
      <section
        aria-labelledby={`permission-${permission.requestId}`}
        className="mb-2 overflow-hidden rounded-xl border border-destructive/40 bg-artifact"
        data-artifact="true"
        role="alert"
      >
        <div className="flex min-h-9 items-center gap-2 border-border/70 border-b px-3">
          <ShieldAlert aria-hidden="true" className="size-4 text-destructive" />
          <h2 className="font-medium text-sm" id={`permission-${permission.requestId}`}>
            Permission details unavailable
          </h2>
        </div>
        <p className="px-3 py-3 text-muted-foreground text-sm">
          Restart or recover the task before deciding this request.
        </p>
      </section>
    )
  }

  async function decide(optionId: string, label: string) {
    if (submittingLabel) return
    setSubmittingLabel(label)
    setError(null)
    const operation = `resolve_permission:${permission.requestId}:${permission.revision}:${optionId}`
    try {
      const frame = await executeCommand(operation, (metadata) => ({
        metadata,
        optionId,
        permissionRequestId: permission.requestId,
        requestRevision: permission.revision,
        taskId,
        type: 'resolve_permission',
      }))
      requireAcceptedCommand(frame, taskId)
    } catch (reason) {
      setSubmittingLabel(null)
      setError(reason instanceof Error ? reason.message : String(reason))
    }
  }

  return (
    <section
      aria-labelledby={`permission-${permission.requestId}`}
      className="mb-2 overflow-hidden rounded-xl border border-state-waiting/40 bg-artifact"
      data-artifact="true"
    >
      <p
        aria-label="Pending permission request"
        aria-live="polite"
        className="sr-only"
        role="status"
      >
        Permission request: {details.preview}
      </p>
      <div className="flex min-h-9 items-center gap-2 border-border/70 border-b px-3">
        <ShieldAlert aria-hidden="true" className="size-4 text-state-waiting" />
        <h2 className="font-medium text-sm" id={`permission-${permission.requestId}`}>
          Permission required
        </h2>
        <span className="ml-auto text-state-waiting text-xs">Waiting permission</span>
      </div>
      <div className="space-y-3 px-3 py-3">
        <pre className="overflow-x-auto whitespace-pre-wrap rounded-md bg-code-background px-3 py-2 font-mono text-xs">
          {details.preview}
        </pre>
        <div className="flex flex-wrap gap-2">
          {details.options.map((option) => (
            <button
              className="rounded-md border border-border bg-surface-raised px-3 py-1.5 font-medium text-sm hover:bg-row-muted disabled:cursor-wait disabled:opacity-60"
              disabled={submittingLabel !== null}
              key={option.optionId}
              onClick={() => void decide(option.optionId, option.label)}
              type="button"
            >
              {option.label}
            </button>
          ))}
        </div>
        {submittingLabel ? (
          <p aria-live="polite" className="text-muted-foreground text-xs" role="status">
            Submitting {submittingLabel}
          </p>
        ) : null}
        {error ? (
          <p className="text-destructive text-xs" role="alert">
            {error}
          </p>
        ) : null}
      </div>
    </section>
  )
}
