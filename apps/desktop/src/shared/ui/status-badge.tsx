import { cva, type VariantProps } from 'class-variance-authority'
import type { ComponentProps } from 'react'

import { cn } from '@/shared/lib/utils'

const statusBadgeVariants = cva(
  'inline-flex w-fit shrink-0 items-center gap-1 rounded-full border border-transparent px-2 py-0.5 font-semibold text-xs tracking-[0.0125em] transition-colors',
  {
    variants: {
      tone: {
        neutral: 'bg-secondary text-secondary-foreground',
        success: 'bg-success/12 text-success',
        warning: 'bg-warning/12 text-warning',
        destructive: 'bg-destructive/12 text-destructive',
        info: 'bg-info/12 text-info',
      },
    },
    defaultVariants: {
      tone: 'neutral',
    },
  },
)

export interface StatusBadgeProps
  extends ComponentProps<'span'>,
    VariantProps<typeof statusBadgeVariants> {}

export function StatusBadge({ className, tone, ...props }: StatusBadgeProps) {
  return <span className={cn(statusBadgeVariants({ tone }), className)} {...props} />
}
