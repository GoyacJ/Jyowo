import type { PermissionRequestBlock } from './conversation-blocks'

export function PermissionRequestBlockView({
  block,
  onResolve,
}: {
  block: PermissionRequestBlock
  onResolve?: (request: {
    conversationId: string
    requestId: string
    decision: 'approve' | 'deny'
  }) => void
}) {
  const disabled = block.status === 'submitting' || block.status === 'resolved'
  const statusLabel =
    block.status === 'resolved'
      ? block.decision === 'approve'
        ? 'Approved'
        : 'Denied'
      : block.status === 'submitting'
        ? 'Submitting'
        : block.status === 'failed'
          ? 'Failed'
          : 'Pending'

  return (
    <section className="ml-12 border-border border-l pl-4">
      <div className="flex items-start justify-between gap-4">
        <div>
          <div className="flex items-center gap-2">
            <h2 className="font-medium text-sm">{block.operation}</h2>
            <span className="rounded-full border border-border px-2 py-0.5 text-muted-foreground text-xs">
              {statusLabel}
            </span>
          </div>
          <p className="mt-1 text-muted-foreground text-sm">{block.reason}</p>
          <dl className="mt-3 grid gap-1 text-xs">
            <div className="flex gap-2">
              <dt className="w-24 text-muted-foreground">Target</dt>
              <dd>{block.target}</dd>
            </div>
            <div className="flex gap-2">
              <dt className="w-24 text-muted-foreground">Risk</dt>
              <dd>{block.severity}</dd>
            </div>
            <div className="flex gap-2">
              <dt className="w-24 text-muted-foreground">Scope</dt>
              <dd>{block.decisionScope}</dd>
            </div>
          </dl>
          {block.errorMessage ? (
            <p className="mt-2 text-destructive text-xs">{block.errorMessage}</p>
          ) : null}
        </div>
        <div className="flex shrink-0 gap-2">
          <button
            className="rounded-md border border-border px-3 py-1.5 text-sm disabled:opacity-60"
            disabled={disabled}
            onClick={() =>
              onResolve?.({
                conversationId: block.conversationId,
                requestId: block.requestId,
                decision: 'deny',
              })
            }
            type="button"
          >
            Deny
          </button>
          <button
            className="rounded-md bg-primary px-3 py-1.5 text-primary-foreground text-sm disabled:opacity-60"
            disabled={disabled}
            onClick={() =>
              onResolve?.({
                conversationId: block.conversationId,
                requestId: block.requestId,
                decision: 'approve',
              })
            }
            type="button"
          >
            Approve
          </button>
        </div>
      </div>
    </section>
  )
}
