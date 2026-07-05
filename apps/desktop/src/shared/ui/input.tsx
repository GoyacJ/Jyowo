import type { InputHTMLAttributes } from 'react'

import { cn } from '@/shared/lib/utils'

export function Input({ className, ...props }: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      className={cn(
        'h-9 w-full rounded-sm border border-input bg-background px-3 py-1 text-sm tracking-normal outline-none transition-colors placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50',
        className,
      )}
      data-slot="input"
      {...props}
    />
  )
}
