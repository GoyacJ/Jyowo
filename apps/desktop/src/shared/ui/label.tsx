import type { LabelHTMLAttributes } from 'react'

import { cn } from '@/shared/lib/utils'

export function Label({ className, ...props }: LabelHTMLAttributes<HTMLLabelElement>) {
  return (
    // biome-ignore lint/a11y/noLabelWithoutControl: shared label receives htmlFor from callers.
    <label
      className={cn('font-medium text-sm tracking-normal', className)}
      data-slot="label"
      {...props}
    />
  )
}
