import { Wrench } from 'lucide-react'

type DeferredToolHint = {
  name: string
  hint?: string | null
}

export type DeferredToolsDelta = {
  added: DeferredToolHint[]
  deferredTotal: number
  removed: string[]
  source?: unknown
}

export function DeferredToolsDeltaCard({ change }: { change: DeferredToolsDelta }) {
  return (
    <div className="rounded-md border border-border px-3 py-2 text-sm">
      <div className="flex items-center gap-2 font-medium">
        <Wrench className="size-3.5 shrink-0 text-muted-foreground" />
        <span>{change.deferredTotal} deferred</span>
      </div>
      <div className="mt-2 grid gap-1.5">
        {change.added.map((tool) => (
          <div key={`added:${tool.name}`} className="flex flex-wrap items-baseline gap-x-2">
            <span className="rounded bg-success/10 px-1.5 py-0.5 font-mono text-success text-xs">
              + <span>{tool.name}</span>
            </span>
            {tool.hint ? <span className="text-muted-foreground text-xs">{tool.hint}</span> : null}
          </div>
        ))}
        {change.removed.map((name) => (
          <div key={`removed:${name}`}>
            <span className="rounded bg-muted px-1.5 py-0.5 font-mono text-muted-foreground text-xs">
              - <span>{name}</span>
            </span>
          </div>
        ))}
      </div>
    </div>
  )
}
