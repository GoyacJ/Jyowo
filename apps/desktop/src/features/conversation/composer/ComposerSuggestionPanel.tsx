import type { ReactNode } from 'react'

import { cn } from '@/shared/lib/utils'

export function ComposerSuggestionPanel({
  children,
  className,
  keyboardHint,
}: {
  children: ReactNode
  className?: string
  keyboardHint: string
}) {
  return (
    <div
      className={cn(
        'absolute bottom-[calc(100%+0.5rem)] left-0 z-50 w-full overflow-hidden rounded-lg border border-border bg-popover text-popover-foreground shadow-deep',
        className,
      )}
      data-testid="composer-suggestion-panel"
    >
      {children}
      <div
        aria-hidden="true"
        className="border-border border-t px-3 py-2 text-muted-foreground text-xs"
      >
        {keyboardHint}
      </div>
    </div>
  )
}
