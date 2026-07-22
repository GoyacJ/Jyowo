import type { InputHTMLAttributes, ReactNode } from 'react'

import { cn } from '@/shared/lib/utils'

type SelectionCardProps = Omit<InputHTMLAttributes<HTMLInputElement>, 'type'> & {
  children: ReactNode
}

function SelectionCard({
  children,
  className,
  disabled,
  type,
  ...props
}: SelectionCardProps & { type: 'checkbox' | 'radio' }) {
  return (
    <label
      className={cn(
        'flex cursor-pointer items-start gap-3 rounded-md border border-border bg-background p-4 transition-[border-color,background-color,box-shadow] duration-200 has-[:checked]:border-primary has-[:checked]:bg-muted/35 has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring',
        disabled && 'cursor-not-allowed opacity-60',
        className,
      )}
      data-slot={`${type}-card`}
    >
      <input className="mt-1 size-4 accent-primary" disabled={disabled} type={type} {...props} />
      <span className="min-w-0 flex-1 space-y-1">{children}</span>
    </label>
  )
}

export function RadioCard(props: SelectionCardProps) {
  return <SelectionCard type="radio" {...props} />
}

export function CheckboxCard(props: SelectionCardProps) {
  return <SelectionCard type="checkbox" {...props} />
}
