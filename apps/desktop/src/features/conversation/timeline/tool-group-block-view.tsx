import { ChevronDown, ChevronRight } from 'lucide-react'
import { useState } from 'react'

import type { ToolGroupBlock } from './conversation-blocks'

export function ToolGroupBlockView({ block }: { block: ToolGroupBlock }) {
  const [expanded, setExpanded] = useState(block.expanded)
  const visibleItems = expanded ? block.items : block.items.slice(0, 2)

  return (
    <section className="ml-12 border-border border-l pl-4">
      <button
        className="flex items-center gap-2 text-left font-medium text-sm"
        onClick={() => setExpanded((current) => !current)}
        type="button"
      >
        {expanded ? <ChevronDown className="size-4" /> : <ChevronRight className="size-4" />}
        Tools
        <span className="text-muted-foreground text-xs">{block.items.length}</span>
      </button>
      <div className="mt-2 grid gap-2">
        {visibleItems.map((item) => (
          <div className="flex items-start justify-between gap-3 text-sm" key={item.id}>
            <div>
              <span>{toolLabel(item.name)}</span>
              {item.argumentsSummary ? (
                <p className="text-muted-foreground text-xs">{item.argumentsSummary}</p>
              ) : null}
              {item.errorMessage ? (
                <p className="text-destructive text-xs">{item.errorMessage}</p>
              ) : null}
            </div>
            <span className="text-muted-foreground text-xs">{item.status}</span>
          </div>
        ))}
      </div>
    </section>
  )
}

function toolLabel(name: string) {
  if (name.includes('read')) {
    return 'Reading files'
  }
  if (name.includes('write') || name.includes('edit')) {
    return 'Updating artifact'
  }
  if (name.includes('command') || name.includes('exec')) {
    return 'Running command'
  }
  return name
}
