import type { DiffReviewBlock } from './conversation-blocks'

export function DiffReviewBlockView({ block }: { block: DiffReviewBlock }) {
  return (
    <section className="ml-12 border-border border-l pl-4">
      <h2 className="font-medium text-sm">{block.title}</h2>
      {block.preview ? (
        <pre className="mt-3 max-h-56 overflow-auto rounded-md bg-code-background p-3 text-xs">
          {block.preview}
        </pre>
      ) : null}
      <p className="mt-2 text-muted-foreground text-xs">{block.status}</p>
    </section>
  )
}
