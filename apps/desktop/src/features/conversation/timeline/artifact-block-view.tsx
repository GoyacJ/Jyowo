import type { ArtifactBlock } from './conversation-blocks'

export function ArtifactBlockView({ block }: { block: ArtifactBlock }) {
  return (
    <section className="ml-12 border-border border-l pl-4">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h2 className="font-medium text-sm">{block.title}</h2>
          {block.description ? (
            <p className="mt-1 text-muted-foreground text-sm">{block.description}</p>
          ) : null}
          {block.preview ? (
            <pre className="mt-3 max-h-48 overflow-auto rounded-md bg-code-background p-3 text-xs">
              {block.preview}
            </pre>
          ) : null}
        </div>
        <span className="text-muted-foreground text-xs">{block.status}</span>
      </div>
      <button className="mt-3 rounded-md border border-border px-3 py-1.5 text-sm" type="button">
        {block.actionLabel}
      </button>
    </section>
  )
}
