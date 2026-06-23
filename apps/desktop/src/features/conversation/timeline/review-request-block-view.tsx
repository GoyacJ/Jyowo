import type { ReviewRequestBlock } from './conversation-blocks'

export function ReviewRequestBlockView({
  block,
  onContinue,
}: {
  block: ReviewRequestBlock
  onContinue?: (prompt: string) => void
}) {
  return (
    <section className="ml-12 border-border border-l pl-4">
      <div className="flex items-center justify-between gap-4">
        <div>
          <h2 className="font-medium text-sm">{block.title}</h2>
          <p className="mt-1 text-muted-foreground text-xs">{block.status}</p>
        </div>
        <button
          className="rounded-md border border-border px-3 py-1.5 text-sm"
          onClick={() => onContinue?.(block.continuePrompt)}
          type="button"
        >
          Continue
        </button>
      </div>
    </section>
  )
}
