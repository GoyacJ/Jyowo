import type { HTMLAttributes } from 'react'

import { cn } from '@/shared/lib/utils'

export function Section({ className, ...props }: HTMLAttributes<HTMLElement>) {
  return (
    <section
      className={cn(
        'space-y-5 rounded-md border border-border bg-surface p-5 shadow-sm',
        className,
      )}
      {...props}
    />
  )
}

export function SectionHeader({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return <div className={cn('space-y-1', className)} {...props} />
}

export function SectionTitle({ className, ...props }: HTMLAttributes<HTMLHeadingElement>) {
  return <h2 className={cn('text-section-title', className)} {...props} />
}

export function SectionDescription({ className, ...props }: HTMLAttributes<HTMLParagraphElement>) {
  return <p className={cn('text-body-muted', className)} {...props} />
}
