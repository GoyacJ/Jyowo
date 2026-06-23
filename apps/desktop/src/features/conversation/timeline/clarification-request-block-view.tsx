import type { ClarificationRequestBlock } from './conversation-blocks'

export function ClarificationRequestBlockView({ block }: { block: ClarificationRequestBlock }) {
  return (
    <section className="ml-12 border-border border-l pl-4">
      <h2 className="font-medium text-sm">Clarification needed</h2>
      <p className="mt-1 text-muted-foreground text-sm">{block.prompt}</p>
    </section>
  )
}
