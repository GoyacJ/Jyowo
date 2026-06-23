import type { UserMessageBlock } from './conversation-blocks'

export function UserMessageBlockView({ block }: { block: UserMessageBlock }) {
  return (
    <article className="grid grid-cols-[36px_minmax(0,1fr)] gap-4">
      <div className="grid size-8 place-items-center rounded-full bg-muted font-medium text-xs">
        Y
      </div>
      <div>
        <div className="flex items-baseline gap-3">
          <span className="font-medium">You</span>
          <span className="text-muted-foreground text-xs">{statusLabel(block.status)}</span>
        </div>
        <p className="mt-2 whitespace-pre-wrap text-sm leading-6">{block.body}</p>
        {block.errorMessage ? (
          <p className="mt-2 text-destructive text-xs">{block.errorMessage}</p>
        ) : null}
      </div>
    </article>
  )
}

function statusLabel(status: UserMessageBlock['status']) {
  switch (status) {
    case 'sending':
      return 'Sending'
    case 'failed':
      return 'Failed'
    case 'sent':
      return 'Sent'
  }
}
