import type { AssistantStreamingBlock } from './conversation-blocks'

export function AssistantStreamingBlockView({ block }: { block: AssistantStreamingBlock }) {
  return (
    <article className="grid grid-cols-[36px_minmax(0,1fr)] gap-4 border-border border-t pt-4">
      <div className="grid size-8 place-items-center rounded-full bg-foreground font-medium text-background text-xs">
        J
      </div>
      <div>
        <div className="flex items-baseline gap-3">
          <span className="font-medium">Jyowo</span>
          <span className="text-muted-foreground text-xs">
            {block.status === 'streaming' ? 'Writing' : 'Interrupted'}
          </span>
        </div>
        <p className="mt-2 max-w-3xl whitespace-pre-wrap text-sm leading-6">
          {block.body}
          {block.status === 'streaming' ? <span aria-hidden="true">|</span> : null}
        </p>
      </div>
    </article>
  )
}
