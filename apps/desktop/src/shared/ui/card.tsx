import type { HTMLAttributes } from 'react'

import { cn } from '@/shared/lib/utils'

export function Card({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      className={cn(
        'rounded-md border border-border bg-surface text-foreground shadow-sm hover:shadow-card transition-[box-shadow,transform] duration-200',
        className,
      )}
      data-slot="card"
      {...props}
    />
  )
}

export function CardHeader({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return <div className={cn('space-y-1.5 p-4', className)} data-slot="card-header" {...props} />
}

export function CardTitle({ className, ...props }: HTMLAttributes<HTMLHeadingElement>) {
  return (
    <h3
      className={cn('font-medium text-base tracking-normal', className)}
      data-slot="card-title"
      {...props}
    />
  )
}

export function CardContent({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return <div className={cn('p-4 pt-0', className)} data-slot="card-content" {...props} />
}
