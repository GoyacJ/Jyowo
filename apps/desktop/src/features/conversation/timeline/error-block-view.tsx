import type { ErrorBlock } from './conversation-blocks'

export function ErrorBlockView({ block }: { block: ErrorBlock }) {
  return (
    <section className="ml-12 border-destructive border-l pl-4">
      <h2 className="font-medium text-destructive text-sm">Run failed</h2>
      <p className="mt-1 text-sm">{block.message}</p>
    </section>
  )
}
