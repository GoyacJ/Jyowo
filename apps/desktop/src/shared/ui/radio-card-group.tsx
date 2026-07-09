import type { InputHTMLAttributes, ReactNode } from 'react'

import { cn } from '@/shared/lib/utils'

type RadioCardProps = Omit<InputHTMLAttributes<HTMLInputElement>, 'type'> & {
  children: ReactNode
}

export function RadioCard({ children, className, disabled, ...props }: RadioCardProps) {
  return (
    <label
      className={cn(
        'flex cursor-pointer items-start gap-3 rounded-md border border-border bg-background p-4 transition-[border-color,background-color,box-shadow] duration-200 has-[:checked]:border-primary has-[:checked]:bg-muted/35 has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring',
        disabled && 'cursor-not-allowed opacity-60',
        className,
      )}
      data-slot="radio-card"
    >
      <input className="mt-1 size-4 accent-primary" disabled={disabled} type="radio" {...props} />
      <span className="space-y-1">{children}</span>
    </label>
  )
}
