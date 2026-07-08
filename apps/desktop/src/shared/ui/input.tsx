import type { InputHTMLAttributes } from 'react'

import { cn } from '@/shared/lib/utils'

export function Input({ className, ...props }: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      className={cn(
        'h-9 w-full rounded-md border border-input bg-background px-3 py-1 text-sm tracking-normal outline-none transition-[border-color,box-shadow] duration-200 placeholder:text-muted-foreground focus:border-ring/60 focus:ring-2 focus:ring-ring/10 disabled:cursor-not-allowed disabled:opacity-50',
        className,
      )}
      data-slot="input"
      {...props}
    />
  )
}
