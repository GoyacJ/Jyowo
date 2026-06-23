import { MarkdownMessage } from '@/shared/markdown/MarkdownMessage'
import type { AssistantMessageBlock } from './conversation-blocks'

export function AssistantMessageBlockView({ block }: { block: AssistantMessageBlock }) {
  return (
    <article className="grid grid-cols-[36px_minmax(0,1fr)] gap-4 border-border border-t pt-4">
      <div className="grid size-8 place-items-center rounded-full bg-foreground font-medium text-background text-xs">
        J
      </div>
      <div>
        <div className="flex items-baseline gap-3">
          <span className="font-medium">Jyowo</span>
          <span className="text-muted-foreground text-xs">
            {block.status === 'partial' ? 'Partial' : 'Complete'}
          </span>
        </div>
        <div className="mt-2 max-w-3xl text-sm leading-6">
          <MarkdownMessage>{block.body}</MarkdownMessage>
        </div>
      </div>
    </article>
  )
}
