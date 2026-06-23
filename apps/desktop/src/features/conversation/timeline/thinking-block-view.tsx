import { useState } from 'react'

import type { ThinkingBlock } from './conversation-blocks'

export function ThinkingBlockView({ block }: { block: ThinkingBlock }) {
  const [collapsed, setCollapsed] = useState(block.collapsed)

  return (
    <section className="ml-12 border-border border-l pl-4">
      <button
        className="flex items-center gap-2 text-left text-muted-foreground text-sm transition-colors hover:text-foreground"
        onClick={() => setCollapsed((value) => !value)}
        type="button"
      >
        <span
          className={
            block.status === 'streaming'
              ? 'inline-flex animate-pulse motion-reduce:animate-none'
              : undefined
          }
        >
          {block.status === 'streaming' ? 'Thinking…' : 'Thought process'}
        </span>
        <span aria-hidden="true">{collapsed ? '▸' : '▾'}</span>
      </button>
      {!collapsed ? (
        <p className="mt-2 max-w-3xl whitespace-pre-wrap text-muted-foreground text-sm leading-6">
          {block.body}
        </p>
      ) : null}
    </section>
  )
}
