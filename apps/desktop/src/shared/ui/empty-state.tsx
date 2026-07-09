import type { HTMLAttributes } from 'react'

import { cn } from '@/shared/lib/utils'

export function EmptyState({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      className={cn(
        'rounded-md border border-dashed border-border bg-background px-4 py-6 text-center text-muted-foreground text-sm',
        className,
      )}
      data-slot="empty-state"
      {...props}
    />
  )
}
